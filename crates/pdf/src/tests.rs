use designcraft_color::swatch::{ColorType, SwatchValue};
use designcraft_color::{Color, Swatch};
use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Document, Fill, Item, ItemId, ParaFormat, SpreadRef};
use designcraft_geom::Rect;

use crate::*;

fn doc_with_text(text: &str) -> Document {
    let mut d = Document::new(&NewDocument { pages: 2, ..NewDocument::default() });
    let lid = d.default_layer();
    d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 576.0, 300.0), lid, text, ParaFormat::default()).unwrap();
    d
}

fn add_box(d: &mut Document, r: Rect, swatch: &str) {
    let lid = d.default_layer();
    let id = ItemId(d.alloc());
    let mut b = Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(r));
    b.fill = Fill::swatch(swatch);
    d.insert_item(SpreadRef::Doc(0), b, None).unwrap();
}

fn uncompressed(d: &Document, opts: PdfOptions) -> String {
    let bytes = export_pdf(d, &Cache::new(), &PdfOptions { compress: false, ..opts }).expect("export");
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Text of every page through hayro's interpreter (glyph → Unicode via ToUnicode / ActualText).
pub(crate) fn extract_text(bytes: &[u8]) -> Vec<String> {
    use hayro_interpret::font::Glyph;
    use hayro_interpret::{
        BlendMode, ClipPath, Context, Device, GlyphDrawMode, Image, InterpreterCache, InterpreterSettings, Paint, PathDrawMode, SoftMask,
        interpret_page,
    };
    use hayro_syntax::Pdf;
    struct Ex(String);
    impl Device<'_> for Ex {
        fn set_soft_mask(&mut self, _: Option<SoftMask<'_>>) {}
        fn set_blend_mode(&mut self, _: BlendMode) {}
        fn draw_path(&mut self, _: &kurbo::BezPath, _: kurbo::Affine, _: &Paint<'_>, _: &PathDrawMode) {}
        fn push_clip_path(&mut self, _: &ClipPath) {}
        fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'_>>, _: BlendMode) {}
        fn draw_glyph(&mut self, g: &Glyph<'_>, _: kurbo::Affine, _: kurbo::Affine, _: &Paint<'_>, _: &GlyphDrawMode) {
            match g.as_unicode() {
                Some(hayro_cmap::BfString::Char(c)) => self.0.push(c),
                Some(hayro_cmap::BfString::String(s)) => self.0.push_str(&s),
                None => self.0.push('\u{FFFD}'),
            }
        }
        fn draw_image(&mut self, _: Image<'_, '_>, _: kurbo::Affine) {}
        fn pop_clip_path(&mut self) {}
        fn pop_transparency_group(&mut self) {}
    }
    let pdf = Pdf::new(bytes.to_vec()).expect("parse");
    let cache = InterpreterCache::new();
    pdf.pages()
        .iter()
        .map(|page| {
            let mut ctx =
                Context::new(kurbo::Affine::IDENTITY, kurbo::Rect::new(0.0, 0.0, 1.0, 1.0), &cache, pdf.xref(), InterpreterSettings::default());
            let mut ex = Ex(String::new());
            interpret_page(page, &mut ctx, &mut ex);
            ex.0
        })
        .collect()
}

#[test]
fn exports_valid_pdf_with_one_page_per_page() {
    let d = doc_with_text("Hello DesignCraft");
    let bytes = export_pdf(&d, &Cache::new(), &PdfOptions::default()).unwrap();
    assert!(bytes.starts_with(b"%PDF-1.7"));
    let pdf = hayro_syntax::Pdf::new(bytes.clone()).expect("parse");
    assert_eq!(pdf.pages().len(), 2);
}

#[test]
fn discretionary_hyphen_pdf_text_follows_visible_breaks() {
    let text = "Wir möchten uns ganz besonders herzlich bei folgenden Personen bedanken, die auf unterschied\u{ad}lichste Weise zur Entstehung dieses Lehrbuches beigetragen haben:";
    for composer in [designcraft_doc::Composer::SingleLine, designcraft_doc::Composer::Paragraph] {
        for (width, expected) in [(430.0, "unterschied-lichste"), (540.0, "unterschiedlichste")] {
            let mut d = Document::new(&NewDocument::default());
            let lid = d.default_layer();
            let para = designcraft_doc::ParaAttrs { composer: Some(composer), hyphenate: Some(false), ..Default::default() };
            let (_, sid) = d
                .add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 36.0 + width, 400.0), lid, text, ParaFormat { para, ..Default::default() })
                .unwrap();
            d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.size = Some(9.5));
            for tagged in [false, true] {
                let bytes = export_pdf(&d, &Cache::new(), &PdfOptions { tagged, ..Default::default() }).unwrap();
                let extracted = extract_text(&bytes).concat();
                assert!(extracted.contains(expected), "{composer:?}, width={width}, tagged={tagged}: {extracted:?}");
                assert!(!extracted.contains('\u{ad}'));
            }
        }
    }
}

#[test]
fn text_is_real_text() {
    let d = doc_with_text("Hello DesignCraft — efficient affine");
    let s = uncompressed(&d, PdfOptions::default());
    assert!(s.contains("/FontFile2") || s.contains("/FontFile3"), "font embedded");
    assert!(s.contains("/ToUnicode"));
    assert!(s.contains(")Tj") || s.contains("]TJ"), "text operators");
    let bytes = export_pdf(&d, &Cache::new(), &PdfOptions::default()).unwrap();
    let text = extract_text(&bytes);
    let flat: String = text[0].split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(flat.contains("Hello"), "{flat}");
    assert!(text[0].contains("DesignCraft"), "{text:?}");
    assert!(text[0].contains("efficient") && text[0].contains("affine"), "ligatures map back: {text:?}");
}

#[test]
fn colours_keep_their_space() {
    let mut d = doc_with_text("x");
    add_box(&mut d, Rect::new(36.0, 400.0, 136.0, 500.0), "C=100 M=0 Y=0 K=0");
    d.swatches.push(Swatch::color("Brand Red", Color::rgb(0.9, 0.1, 0.1)));
    add_box(&mut d, Rect::new(200.0, 400.0, 300.0, 500.0), "Brand Red");
    d.swatches.push(Swatch {
        name: "PANTONE Test".into(),
        value: SwatchValue::Color { color: Color::cmyk(0.0, 0.5, 1.0, 0.0), color_type: ColorType::Spot },
        locked: false,
        named: true,
        hidden: false,
    });
    add_box(&mut d, Rect::new(320.0, 400.0, 420.0, 500.0), "PANTONE Test");
    let s = uncompressed(&d, PdfOptions::default());
    assert!(s.contains("1 0 0 0 k"), "DeviceCMYK fill");
    assert!(s.contains(" rg"), "DeviceRGB fill");
    assert!(s.contains("/Separation") && s.contains("/PANTONE#20Test"), "spot as Separation");
}

/// [Paper] went into print PDFs as `1 1 1 rg`, DeviceRGB white.
#[test]
fn paper_prints_as_no_ink() {
    let mut d = doc_with_text("x");
    add_box(&mut d, Rect::new(36.0, 400.0, 136.0, 500.0), "[Paper]");
    let s = uncompressed(&d, PdfOptions::default());
    assert!(s.contains("0 0 0 0 k"), "CMYK, no ink");
    assert!(!s.contains("1 1 1 rg"), "no RGB white");
    // A web document keeps the paper colour.
    d.settings.intent = designcraft_doc::Intent::Web;
    assert!(uncompressed(&d, PdfOptions::default()).contains("1 1 1 rg"));
    // PDF/X has a CMYK output intent whatever the document's intent.
    assert!(!uncompressed(&d, PdfOptions { standard: Standard::PdfX4, ..Default::default() }).contains("1 1 1 rg"));
}

#[test]
fn boxes_bleed_and_marks() {
    let mut d = doc_with_text("x");
    d.settings.bleed = [9.0; 4];
    let s = uncompressed(&d, PdfOptions { bleed: true, ..Default::default() });
    assert!(s.contains("/TrimBox[9 9 621 801]"), "{}", &s[..s.len().min(4000)]);
    assert!(s.contains("/MediaBox[0 0 630 810]"));
    assert!(s.contains("/BleedBox[0 0 630 810]"));
    let s = uncompressed(&d, PdfOptions { bleed: true, marks: Marks::ALL, pages: Some(vec![1]), ..Default::default() });
    // margin = max(offset 6, bleed 9) + 18 + 6 = 33
    assert!(s.contains("/MediaBox[0 0 678 858]"), "marks area");
    assert!(s.contains("/All"), "registration colour");
    let bytes = export_pdf(&d, &Cache::new(), &PdfOptions { marks: Marks::ALL, pages: Some(vec![1]), ..Default::default() }).unwrap();
    let t = extract_text(&bytes);
    assert_eq!(t.len(), 1);
    assert!(t[0].contains("Page 2"), "{t:?}");
}

#[test]
fn spreads_and_ranges() {
    let mut d = Document::new(&NewDocument { pages: 4, facing_pages: true, ..NewDocument::default() });
    let _ = &mut d;
    let n = d.page_count();
    assert_eq!(n, 4);
    let bytes = export_pdf(&d, &Cache::new(), &PdfOptions { spreads: true, ..Default::default() }).unwrap();
    let pages = hayro_syntax::Pdf::new(bytes).unwrap().pages().len();
    assert_eq!(pages, d.spreads.len());
    assert_eq!(parse_page_range("1-2, 4", 4).unwrap(), vec![0, 1, 3]);
    assert_eq!(parse_page_range("3-", 4).unwrap(), vec![2, 3]);
    assert_eq!(parse_page_range("", 2).unwrap(), vec![0, 1]);
    // Ranges that overlap many times over: each page is selected once, in the order first named.
    let overlapping = std::iter::repeat_n("1-20000, 5-19000", 500).collect::<Vec<_>>().join(",");
    let t = std::time::Instant::now();
    assert_eq!(parse_page_range(&overlapping, 20000).unwrap().len(), 20000);
    assert!(t.elapsed() < std::time::Duration::from_secs(5), "{:?}", t.elapsed());
    assert_eq!(parse_page_range("3, 1-3", 4).unwrap(), vec![2, 0, 1]);
    assert!(parse_page_range("5", 4).is_err());
    assert!(parse_page_range("x", 4).is_err());
    let bytes = export_pdf(&d, &Cache::new(), &PdfOptions { pages: Some(vec![1, 2]), ..Default::default() }).unwrap();
    assert_eq!(hayro_syntax::Pdf::new(bytes).unwrap().pages().len(), 2);
    assert_eq!(export_pdf(&d, &Cache::new(), &PdfOptions { pages: Some(vec![9]), ..Default::default() }), Err(PdfError::BadPage(10)));
}

#[test]
fn pdfx4_xmp_has_metadata_date_and_version_id() {
    let d = doc_with_text("Print me");
    let opts = PdfOptions { standard: Standard::PdfX4, created: Some(1_700_000_000), ..Default::default() };
    let r = export_pdf_with_report(&d, &Cache::new(), &opts).unwrap();
    let text = String::from_utf8_lossy(&r.bytes).to_string();
    assert!(text.contains("<xmp:MetadataDate>2023-11-14T22:13:20Z</xmp:MetadataDate>"), "metadata date");
    assert!(text.contains("<xmpMM:VersionID>1</xmpMM:VersionID>"), "version id");
    assert!(crate::check_pdfx4(&r.bytes).is_empty());
    // Without the two entries the check says so.
    let stripped = text.replace("xmp:MetadataDate", "xmp:Other").replace("xmpMM:VersionID", "xmpMM:Other");
    assert_eq!(crate::check_pdfx4(stripped.as_bytes()).iter().filter(|i| i.contains("xmp")).count(), 2);
}

#[test]
fn standards() {
    let d = doc_with_text("Archive me");
    let r = export_pdf_with_report(&d, &Cache::new(), &PdfOptions { standard: Standard::PdfA2b, created: Some(1_700_000_000), ..Default::default() });
    let r = r.expect("PDF/A-2b export");
    assert!(String::from_utf8_lossy(&r.bytes).contains("pdfaid"));
    let r = export_pdf_with_report(&d, &Cache::new(), &PdfOptions { standard: Standard::PdfX4, ..Default::default() }).unwrap();
    assert!(r.bytes.starts_with(b"%PDF-1.6"));
    // Output intent, identification and checks all in place; the update still reads.
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert!(crate::check_pdfx4(&r.bytes).is_empty());
    // The default working CMYK space is the built-in Coated FOGRA39.
    let intent = crate::print_tests::output_intent(&r.bytes).expect("output intent");
    assert_eq!((intent.identifier.as_str(), intent.condition.as_str()), ("FOGRA39", "Coated FOGRA39 (fitted, craft-cms)"));
    // A plain export fails the checks; both read with the same pages.
    let plain = export_pdf(&d, &Cache::new(), &PdfOptions::default()).unwrap();
    assert_eq!(hayro_syntax::Pdf::new(r.bytes.clone()).unwrap().pages().len(), hayro_syntax::Pdf::new(plain.clone()).unwrap().pages().len());
    assert!(crate::check_pdfx4(&plain).len() >= 3);
    assert_eq!(Standard::parse("PDF/X-4"), Some(Standard::PdfX4));
    assert_eq!(Standard::parse("pdfa-2b"), Some(Standard::PdfA2b));
}

#[test]
fn table_gradient_fill_exports_a_shading() {
    let mut d = Document::new(&NewDocument::default());
    let mut gradient = designcraft_color::Gradient::default();
    gradient.stops[0].color = Color::rgb(1.0, 0.0, 0.0);
    gradient.stops[1].color = Color::rgb(0.0, 0.0, 1.0);
    d.swatches.push(Swatch { name: "Table gradient".into(), value: SwatchValue::Gradient { gradient }, locked: false, named: true, hidden: false });
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 336.0, 300.0), d.default_layer(), "", ParaFormat::default()).unwrap();
    let mut t = designcraft_doc::Table::new(1, 1, 2, 0, 0, 200.0);
    t.rows[0].height = 40.0;
    t.rows[0].mode = designcraft_doc::RowHeightMode::Exactly;
    for cell in &mut t.cells {
        cell.fill = "Table gradient".into();
    }
    d.story_mut(sid).unwrap().insert_table(0, t);
    let pdf = uncompressed(&d, PdfOptions::default());
    assert!(pdf.contains("/ShadingType 2") || pdf.contains("/ShadingType2"), "table fill must export a linear gradient");
}

#[test]
fn tables_export_cell_text() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 336.0, 400.0), lid, "Before", ParaFormat::default()).unwrap();
    let mut t = designcraft_doc::Table::new(9, 2, 2, 1, 0, 300.0);
    t.cell_mut(0, 0).unwrap().text.insert(0, "Ink");
    t.cell_mut(0, 0).unwrap().fill = "C=100 M=0 Y=0 K=0".into();
    t.cell_mut(1, 1).unwrap().text.insert(0, "Plum");
    t.cell_mut(2, 0).unwrap().text.insert(0, "Sunset");
    d.story_mut(sid).unwrap().insert_table(6, t);
    let bytes = export_pdf(&d, &Cache::new(), &PdfOptions::default()).unwrap();
    let text = extract_text(&bytes).concat();
    for w in ["Before", "Ink", "Plum", "Sunset"] {
        assert!(text.contains(w), "{w} missing from {text:?}");
    }
}

#[test]
fn drop_cap_text_extracts_in_order() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let para = designcraft_doc::ParaAttrs { drop_cap_lines: Some(3), drop_cap_chars: Some(2), ..Default::default() };
    let text = "Drop caps read in order with the rest of the paragraph, which runs on for a few lines beside the enlarged letters.";
    d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 236.0, 300.0), lid, text, ParaFormat { para, ..Default::default() }).unwrap();
    let bytes = export_pdf(&d, &Cache::new(), &PdfOptions::default()).unwrap();
    let flat: String = extract_text(&bytes)[0].split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(flat.starts_with("Drop caps read in order"), "{flat}");
}

/// A 4×2 CMYK TIFF: the left half 100% K (text in a scan), the right half C0 M100 Y100 K0.
fn cmyk_tiff() -> (Vec<u8>, Vec<u8>) {
    let (k, red) = ([0u8, 0, 0, 255], [0u8, 255, 255, 0]);
    let px: Vec<u8> = (0..2).flat_map(|_| [k, k, red, red]).flatten().collect();
    let mut b = Vec::new();
    tiff::encoder::TiffEncoder::new(std::io::Cursor::new(&mut b)).unwrap().write_image::<tiff::encoder::colortype::CMYK8>(4, 2, &px).unwrap();
    (b, px)
}

/// A black `side`² 1-bit grey PNG: well under 1 MB, it decodes to `side`² bytes.
pub(crate) fn blank_png(side: u32) -> Vec<u8> {
    use std::io::Write;
    let chunk = |out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]| {
        out.extend((data.len() as u32).to_be_bytes());
        let mut crc = flate2::Crc::new();
        crc.update(kind);
        crc.update(data);
        out.extend(kind);
        out.extend(data);
        out.extend(crc.sum().to_be_bytes());
    };
    let row = vec![0u8; 1 + side.div_ceil(8) as usize];
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    for _ in 0..side {
        z.write_all(&row).unwrap();
    }
    let mut ihdr = [side.to_be_bytes(), side.to_be_bytes()].concat();
    ihdr.extend([1, 0, 0, 0, 0]);
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &z.finish().unwrap());
    chunk(&mut png, b"IEND", &[]);
    png
}

fn place(d: &mut Document, data: Vec<u8>, px: (u32, u32)) {
    place_at(d, data, px, designcraft_geom::Affine::translate((72.0, 72.0)) * designcraft_geom::Affine::scale(50.0));
}

/// `place` with the graphic's transform in its frame.
fn place_at(d: &mut Document, data: Vec<u8>, px: (u32, u32), xf: designcraft_geom::Affine) {
    let asset = designcraft_doc::AssetId(d.alloc());
    let mime = designcraft_images::mime(&data).into();
    d.assets.insert(
        asset,
        std::sync::Arc::new(designcraft_doc::Asset {
            id: asset,
            name: "scan.tif".into(),
            mime,
            data: data.into(),
            pixels: Some(px),
            ..Default::default()
        }),
    );
    let lid = d.default_layer();
    let id = ItemId(d.alloc());
    let mut it = Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(72.0, 72.0, 272.0, 172.0)));
    it.content = designcraft_doc::Content::Graphic(designcraft_doc::Graphic {
        layer_options: None,
        asset,
        size: (px.0 as f64, px.1 as f64),
        xf,
        clip: None,
        auto_fit: designcraft_doc::Fitting::FillProportionally,
        fit_align: 4,
        crop: [0.0; 4],
        wrap: Default::default(),
        fill: Default::default(),
    });
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
}

/// A placed image whose linked file wasn't found has no data: the warning says it is missing,
/// not that it could not be decoded.
#[test]
fn a_missing_linked_image_is_reported_as_missing() {
    let mut d = doc_with_text("still here");
    place(&mut d, Vec::new(), (64, 64));
    if let Some(a) = d.assets.values_mut().next() {
        std::sync::Arc::make_mut(a).link = Some("/gone/curtains.jpeg".into());
    }
    let r = export_pdf_with_report(&d, &Cache::new(), &PdfOptions::default()).unwrap();
    assert!(r.warnings.iter().any(|w| w.contains("is missing (/gone/curtains.jpeg) and was skipped")), "{:?}", r.warnings);
    assert!(!r.warnings.iter().any(|w| w.contains("could not be decoded")), "{:?}", r.warnings);
}

/// krilla reads PNG, GIF and WebP lazily, so a damaged one passed the exporter's "does it decode"
/// check and failed the whole export when the PDF was written ("PDF writer error: … unexpected
/// end of file"). A cut-off download must be skipped with a warning, as a damaged TIFF is.
#[test]
fn a_cut_off_image_is_skipped_not_fatal() {
    let px = image::RgbaImage::from_fn(64, 64, |x, y| image::Rgba([(x * 37 + y * 11) as u8, (x * 5) as u8, (y * 91) as u8, 255]));
    for format in [image::ImageFormat::Png, image::ImageFormat::Gif, image::ImageFormat::WebP] {
        let mut whole = Vec::new();
        px.write_to(&mut std::io::Cursor::new(&mut whole), format).unwrap();
        let cut = whole[..whole.len() * 9 / 10].to_vec();
        let mut d = doc_with_text("still here");
        place(&mut d, cut, (64, 64));
        let r = export_pdf_with_report(&d, &Cache::new(), &PdfOptions::default()).unwrap_or_else(|e| panic!("{format:?}: {e}"));
        assert!(r.warnings.iter().any(|w| w.contains("could not be decoded and was skipped")), "{format:?}: {:?}", r.warnings);
        assert!(image_xobjects(&r.bytes).is_empty(), "{format:?}");
        assert!(extract_text(&r.bytes).concat().contains("still"), "{format:?}: the rest of the page is exported");
        // The undamaged file still goes in.
        let mut d = doc_with_text("x");
        place(&mut d, whole, (64, 64));
        let r = export_pdf_with_report(&d, &Cache::new(), &PdfOptions::default()).unwrap();
        assert!(r.warnings.is_empty(), "{format:?}: {:?}", r.warnings);
        assert_eq!(image_xobjects(&r.bytes).len(), 1, "{format:?}");
    }
}

/// A raster past the RGBA budget is skipped with a warning, also when images are recompressed (#1586).
#[test]
fn an_image_past_the_decode_budget_is_skipped_with_a_warning() {
    let side = 16_385;
    let mut d = doc_with_text("still here");
    place(&mut d, blank_png(side), (side, side));
    for compress_images in [false, true] {
        let r = export_pdf_with_report(&d, &Cache::new(), &PdfOptions { compress_images, ..Default::default() }).unwrap();
        assert!(r.warnings.iter().any(|w| w.contains("could not be decoded and was skipped")), "compress_images {compress_images}: {:?}", r.warnings);
        assert!(image_xobjects(&r.bytes).is_empty(), "compress_images {compress_images}");
    }
}

/// Every image XObject in a PDF: its /ColorSpace name (`?` when not a name) and decoded samples.
fn image_xobjects(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    use hayro_syntax::object::Name;
    let pdf = hayro_syntax::Pdf::new(bytes.to_vec()).expect("parse");
    pdf.objects()
        .into_iter()
        .filter_map(|o| o.into_stream())
        .filter(|s| s.dict().get::<Name>("Subtype").is_some_and(|n| n.as_str() == "Image"))
        .map(|s| {
            let cs = s.dict().get::<Name>("ColorSpace").map_or("?".to_string(), |n| n.as_str().to_string());
            (cs, s.decoded().expect("decode").into_owned())
        })
        .collect()
}

/// A placed CMYK TIFF went into PDFs as DeviceRGB (#35): 100% K became RGB black, which the
/// press separates as four-colour black.
#[test]
fn cmyk_tiff_keeps_its_inks() {
    let (tif, px) = cmyk_tiff();
    let mut d = doc_with_text("x");
    place(&mut d, tif, (4, 2));
    for standard in [Standard::None, Standard::PdfX4] {
        let r = export_pdf_with_report(&d, &Cache::new(), &PdfOptions { standard, ..Default::default() }).unwrap();
        assert!(r.warnings.is_empty(), "{standard:?}: {:?}", r.warnings);
        let images = image_xobjects(&r.bytes);
        assert_eq!(images, vec![("DeviceCMYK".to_string(), px.clone())], "{standard:?}");
        assert_eq!(images[0].1[..4], [0, 0, 0, 255], "K only");
    }
}

/// A placed greyscale TIFF went into PDFs as DeviceRGB, unlike a greyscale PNG or JPEG (#1426):
/// a page holding only a black-and-white scan declared colour.
#[test]
fn grey_tiff_exports_as_device_gray() {
    let px: Vec<u8> = vec![0, 64, 128, 255, 255, 128, 64, 0];
    let mut tif = Vec::new();
    tiff::encoder::TiffEncoder::new(std::io::Cursor::new(&mut tif)).unwrap().write_image::<tiff::encoder::colortype::Gray8>(4, 2, &px).unwrap();
    let mut d = doc_with_text("x");
    place(&mut d, tif, (4, 2));
    for standard in [Standard::None, Standard::PdfX4] {
        let r = export_pdf_with_report(&d, &Cache::new(), &PdfOptions { standard, ..Default::default() }).unwrap();
        assert!(r.warnings.is_empty(), "{standard:?}: {:?}", r.warnings);
        assert_eq!(image_xobjects(&r.bytes), vec![("DeviceGray".to_string(), px.clone())], "{standard:?}");
    }
    // Recompressed, it stays a grey JPEG.
    let r = export_pdf_with_report(&d, &Cache::new(), &PdfOptions { compress_images: true, compress: false, ..Default::default() }).unwrap();
    let s = String::from_utf8_lossy(&r.bytes);
    assert!(s.contains("/DCTDecode") && s.contains("/DeviceGray") && !s.contains("/DeviceRGB"), "a grey JPEG");
}

/// An 8×8 raster encoded as `fmt`; `alpha` gives it a transparent corner.
fn raster(fmt: image::ImageFormat, alpha: bool) -> Vec<u8> {
    let img =
        image::RgbaImage::from_fn(8, 8, |x, y| image::Rgba([(x * 30) as u8, (y * 30) as u8, 128, if alpha && x == 0 && y == 0 { 0 } else { 255 }]));
    let img = if fmt == image::ImageFormat::Jpeg {
        image::DynamicImage::ImageRgb8(image::DynamicImage::ImageRgba8(img).to_rgb8())
    } else {
        image::DynamicImage::ImageRgba8(img)
    };
    let mut b = std::io::Cursor::new(Vec::new());
    img.write_to(&mut b, fmt).unwrap();
    b.into_inner()
}

/// Each image XObject's /Interpolate value (`None` when absent).
fn interpolate_flags(bytes: &[u8]) -> Vec<Option<bool>> {
    use hayro_syntax::object::Name;
    let pdf = hayro_syntax::Pdf::new(bytes.to_vec()).expect("parse");
    pdf.objects()
        .into_iter()
        .filter_map(|o| o.into_stream())
        .filter(|s| s.dict().get::<Name>("Subtype").is_some_and(|n| n.as_str() == "Image"))
        .map(|s| s.dict().get::<bool>("Interpolate"))
        .collect()
}

/// PDF/A-2b rejected placed PNG, JPEG, GIF and WebP images (#71): they were written with
/// /Interpolate true, which PDF/A forbids. Ordinary exports keep interpolation.
#[test]
fn archival_exports_keep_placed_images_without_interpolation() {
    use image::ImageFormat as F;
    let mut d = doc_with_text("x");
    for (fmt, alpha) in [(F::Png, true), (F::Png, false), (F::Jpeg, false), (F::Gif, true), (F::WebP, true)] {
        place(&mut d, raster(fmt, alpha), (8, 8));
    }
    for compress_images in [false, true] {
        let opts = PdfOptions { compress_images, created: Some(1_700_000_000), ..Default::default() };
        let a = export_pdf_with_report(&d, &Cache::new(), &PdfOptions { standard: Standard::PdfA2b, ..opts.clone() });
        let a = a.unwrap_or_else(|e| panic!("PDF/A-2b with images (compress {compress_images}): {e:?}"));
        let flags = interpolate_flags(&a.bytes);
        assert!(flags.len() >= 5, "every placed image is in the PDF/A file: {flags:?}");
        assert!(flags.iter().all(|f| *f != Some(true)), "{flags:?}");
        let plain = export_pdf(&d, &Cache::new(), &opts).unwrap();
        let flags = interpolate_flags(&plain);
        assert!(flags.len() >= 5, "{flags:?}");
        assert!(flags.contains(&Some(true)), "ordinary exports interpolate: {flags:?}");
    }
}

/// What a page draws: each glyph (its text and outline bounds) and each filled path's bounds.
fn drawn(bytes: &[u8]) -> (Vec<(String, kurbo::Rect)>, Vec<kurbo::Rect>) {
    use hayro_interpret::font::Glyph;
    use hayro_interpret::{
        BlendMode, ClipPath, Context, Device, GlyphDrawMode, Image, InterpreterCache, InterpreterSettings, Paint, PathDrawMode, SoftMask,
        interpret_page,
    };
    use kurbo::Shape as _;
    #[derive(Default)]
    struct Rec(Vec<(String, kurbo::Rect)>, Vec<kurbo::Rect>);
    impl Device<'_> for Rec {
        fn set_soft_mask(&mut self, _: Option<SoftMask<'_>>) {}
        fn set_blend_mode(&mut self, _: BlendMode) {}
        fn draw_path(&mut self, p: &kurbo::BezPath, xf: kurbo::Affine, _: &Paint<'_>, _: &PathDrawMode) {
            self.1.push((xf * p.clone()).bounding_box());
        }
        fn push_clip_path(&mut self, _: &ClipPath) {}
        fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'_>>, _: BlendMode) {}
        fn draw_glyph(&mut self, g: &Glyph<'_>, _: kurbo::Affine, _: kurbo::Affine, _: &Paint<'_>, _: &GlyphDrawMode) {
            let text = match g.as_unicode() {
                Some(hayro_cmap::BfString::Char(c)) => c.to_string(),
                Some(hayro_cmap::BfString::String(s)) => s,
                None => String::new(),
            };
            let bounds = match g {
                Glyph::Outline(o) => o.outline().bounding_box(),
                Glyph::Type3(_) => kurbo::Rect::ZERO,
            };
            self.0.push((text, bounds));
        }
        fn draw_image(&mut self, _: Image<'_, '_>, _: kurbo::Affine) {}
        fn pop_clip_path(&mut self) {}
        fn pop_transparency_group(&mut self) {}
    }
    let pdf = hayro_syntax::Pdf::new(bytes.to_vec()).expect("parse");
    let cache = InterpreterCache::new();
    let page = &pdf.pages()[0];
    let mut ctx = Context::new(kurbo::Affine::IDENTITY, kurbo::Rect::new(0.0, 0.0, 1.0, 1.0), &cache, pdf.xref(), InterpreterSettings::default());
    let mut rec = Rec::default();
    interpret_page(page, &mut ctx, &mut rec);
    (rec.0, rec.1)
}

/// The page's paint operations in order: `fill`/`stroke` + ` text`/` path` and the stroke width.
fn paint_order(bytes: &[u8]) -> Vec<(String, f32)> {
    paint_events(bytes, false)
}

/// [`paint_order`], with transparency groups when `groups`: `group` (and its opacity) and
/// `end group`.
fn paint_events(bytes: &[u8], groups: bool) -> Vec<(String, f32)> {
    use hayro_interpret::font::Glyph;
    use hayro_interpret::{
        BlendMode, ClipPath, Context, Device, GlyphDrawMode, Image, InterpreterCache, InterpreterSettings, Paint, PathDrawMode, SoftMask,
        interpret_page,
    };
    #[derive(Default)]
    struct Rec(Vec<(String, f32)>, bool);
    impl Device<'_> for Rec {
        fn set_soft_mask(&mut self, _: Option<SoftMask<'_>>) {}
        fn set_blend_mode(&mut self, _: BlendMode) {}
        fn draw_path(&mut self, _: &kurbo::BezPath, _: kurbo::Affine, _: &Paint<'_>, m: &PathDrawMode) {
            self.0.push(match m {
                PathDrawMode::Fill(_) => ("fill path".into(), 0.0),
                PathDrawMode::Stroke(p) => ("stroke path".into(), p.line_width),
            });
        }
        fn push_clip_path(&mut self, _: &ClipPath) {}
        fn push_transparency_group(&mut self, opacity: f32, _: Option<SoftMask<'_>>, _: BlendMode) {
            if self.1 {
                self.0.push(("group".into(), opacity));
            }
        }
        fn draw_glyph(&mut self, _: &Glyph<'_>, _: kurbo::Affine, _: kurbo::Affine, _: &Paint<'_>, m: &GlyphDrawMode) {
            self.0.push(match m {
                GlyphDrawMode::Fill => ("fill text".into(), 0.0),
                GlyphDrawMode::Stroke(p) => ("stroke text".into(), p.line_width),
                GlyphDrawMode::Invisible => ("invisible text".into(), 0.0),
                GlyphDrawMode::Clip => ("clip text".into(), 0.0),
            });
        }
        fn draw_image(&mut self, _: Image<'_, '_>, _: kurbo::Affine) {}
        fn pop_clip_path(&mut self) {}
        fn pop_transparency_group(&mut self) {
            if self.1 {
                self.0.push(("end group".into(), 0.0));
            }
        }
    }
    let pdf = hayro_syntax::Pdf::new(bytes.to_vec()).expect("parse");
    let cache = InterpreterCache::new();
    let page = &pdf.pages()[0];
    let mut ctx = Context::new(kurbo::Affine::IDENTITY, kurbo::Rect::new(0.0, 0.0, 1.0, 1.0), &cache, pdf.xref(), InterpreterSettings::default());
    let mut rec = Rec(Vec::new(), groups);
    interpret_page(page, &mut ctx, &mut rec);
    rec.0
}

#[test]
fn stroke_behind_fill_strokes_the_run_before_filling_it() {
    let export = |behind: bool| {
        let mut d = doc_with_text("I");
        let sid = *d.stories.keys().next().unwrap();
        d.story_mut(sid).unwrap().format_chars(0..1, |f| {
            f.over.stroke = Some(designcraft_color::swatch::BLACK.into());
            f.over.stroke_weight = Some(4.0);
            f.over.stroke_behind_fill = Some(behind);
        });
        let bytes = export_pdf(&d, &Cache::new(), &PdfOptions::default()).unwrap();
        let ops: Vec<(String, f32)> = paint_order(&bytes).into_iter().filter(|(op, w)| op.ends_with("text") || *w > 0.0).collect();
        (ops, extract_text(&bytes)[0].clone())
    };
    let (over, _) = export(false);
    assert_eq!(over, [("fill text".to_string(), 0.0), ("stroke text".to_string(), 4.0)]);
    let (behind, text) = export(true);
    assert_eq!(behind, [("stroke path".to_string(), 8.0), ("fill text".to_string(), 0.0)], "the double-width stroke goes first");
    assert_eq!(text.trim(), "I", "the text is not duplicated");
}

#[test]
fn pdfa_leaves_out_form_fields_added_after_validation() {
    let mut d = doc_with_text("Archive me");
    let lid = d.default_layer();
    let id = ItemId(d.alloc());
    let mut b = Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(36.0, 400.0, 200.0, 430.0)));
    b.form_field = Some(designcraft_doc::FormField {
        kind: designcraft_doc::FieldKind::TextField,
        name: "name".into(),
        value: String::new(),
        options: Vec::new(),
        multiline: false,
        required: false,
        font_size: 0.0,
    });
    d.insert_item(SpreadRef::Doc(0), b, None).unwrap();
    let plain = export_pdf(&d, &Cache::new(), &PdfOptions::default()).unwrap();
    assert!(String::from_utf8_lossy(&plain).contains("/AcroForm"), "control: a plain export has the field");
    let opts = PdfOptions { standard: Standard::PdfA2b, created: Some(1_700_000_000), ..Default::default() };
    let a = export_pdf_with_report(&d, &Cache::new(), &opts).unwrap();
    let text = String::from_utf8_lossy(&a.bytes).to_string();
    assert!(text.contains("pdfaid"));
    assert!(!text.contains("/AcroForm") && !text.contains("NeedAppearances"), "PDF/A must not get the form update");
    assert!(a.warnings.iter().any(|w| w.contains("PDF/A-2b") && w.contains("left out")), "{:?}", a.warnings);
}

#[test]
fn missing_glyphs_print_as_boxes() {
    // Source Serif 4 lacks 語: its .notdef box is drawn as a path, not as a .notdef glyph
    // (which PDF/A and PDF/UA forbid).
    let boxes = |paths: &[kurbo::Rect]| paths.iter().filter(|r| r.width() > 1.0 && r.width() < 12.0 && r.height() > 3.0 && r.height() < 12.0).count();
    let d = doc_with_text("a語b");
    assert!(!d.settings.glyph_fallback);
    let (glyphs, paths) = drawn(&export_pdf(&d, &Cache::new(), &PdfOptions::default()).unwrap());
    assert_eq!(glyphs.iter().map(|g| g.0.as_str()).collect::<Vec<_>>(), ["a", "b"], "{glyphs:?}");
    assert_eq!(boxes(&paths), 1, "{paths:?}");
    let a = export_pdf_with_report(&d, &Cache::new(), &PdfOptions { standard: Standard::PdfA2b, ..Default::default() });
    assert!(a.is_ok(), "PDF/A takes the box: {:?}", a.err());
    // A font whose .notdef is empty: the box DesignCraft draws for it.
    const FAMILY: &str = "DC Test PDF Empty Notdef";
    let font = designcraft_fonts::testing::font_mapping(FAMILY, &[('a', 'a'), ('b', 'b')]).unwrap();
    designcraft_fonts::FontDb::global().add_font(designcraft_fonts::testing::with_empty_notdef(font).unwrap());
    let mut d = doc_with_text("a語b");
    let sid = *d.stories.keys().next().unwrap();
    d.story_mut(sid).unwrap().format_chars(0.."a語b".len(), |f| f.over.font_family = Some(FAMILY.into()));
    let (glyphs, paths) = drawn(&export_pdf(&d, &Cache::new(), &PdfOptions::default()).unwrap());
    assert_eq!(glyphs.iter().map(|g| g.0.as_str()).collect::<Vec<_>>(), ["a", "b"], "{glyphs:?}");
    assert_eq!(boxes(&paths), 1, "{paths:?}");
}

#[test]
fn exports_embedded_text_from_footnotes() {
    use designcraft_doc::{AnchorPosition, AnchoredObject};
    use std::sync::Arc;
    let mut d = doc_with_text("Body");
    let body = *d.stories.keys().next().unwrap();
    let (fid, _) =
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 100.0, 40.0), d.default_layer(), "Embedded", ParaFormat::default()).unwrap();
    let item = d.item(fid).unwrap().clone();
    d.spread_mut(SpreadRef::Doc(0)).unwrap().items.retain(|i| i.id != fid);
    let story = d.story_mut(body).unwrap();
    story.insert_note(4, "Note", ParaFormat::default());
    Arc::make_mut(&mut story.notes[0]).text.insert_object(0, AnchoredObject::new(item, AnchorPosition::Inline { y_offset: 0.0 }));
    d.check().unwrap();
    let bytes = export_pdf(&d, &Cache::new(), &PdfOptions::default()).unwrap();
    let text = extract_text(&bytes).join("");
    assert!(text.contains("Embedded"), "{text}");
}

#[test]
fn fills_open_contours_without_changing_strokes() {
    let mut d = Document::new(&NewDocument::default());
    add_box(&mut d, Rect::new(40.0, 50.0, 62.0, 67.0), designcraft_color::swatch::BLACK);
    let id = d.spreads[0].items[0].id;
    d.item_mut(id).unwrap().path.subpaths[0].closed = false;
    let (_, paths) = drawn(&export_pdf(&d, &Cache::new(), &PdfOptions::default()).unwrap());
    assert!(paths.iter().any(|r| (r.width() - 22.0).abs() < 0.01 && (r.height() - 17.0).abs() < 0.01), "{paths:?}");
}

#[test]
fn column_rules_are_drawn() {
    let rules = |on: bool| {
        let mut d = doc_with_text("Column text");
        let fid = d.stories.values().next().unwrap().frames[0];
        let o = &mut d.item_mut(fid).unwrap().text_frame_mut().unwrap().options;
        o.columns = 3;
        o.gutter = 12.0;
        o.inset = [0.0; 4];
        o.column_rule = on;
        o.column_rule_weight = 2.0;
        // The rules run from the top line's ascent to the bottom line's descent, not the 264 pt
        // of the columns.
        let sid = *d.stories.keys().next().unwrap();
        let cs = designcraft_compose::compose_story(&d, sid, &designcraft_compose::ComposeOptions::default());
        let line = &cs.frames[0].lines[0];
        let depth = line.ascent + line.descent;
        assert!(cs.frames[0].lines.len() == 1 && depth < 264.0 - 1.0, "{depth}");
        let (_, paths) = drawn(&export_pdf(&d, &Cache::new(), &PdfOptions::default()).unwrap());
        paths.into_iter().filter(|r| (r.width() - 2.0).abs() < 0.01 && (r.height() - depth).abs() < 0.01).map(|r| r.center().x).collect::<Vec<_>>()
    };
    // 540 pt wide in three columns with 12 pt gutters: columns of 172 pt from x 36.
    let xs = rules(true);
    assert_eq!(xs.len(), 2, "{xs:?}");
    assert!((xs[0] - 214.0).abs() < 0.01 && (xs[1] - 398.0).abs() < 0.01, "{xs:?}");
    assert!(rules(false).is_empty());
}

#[test]
fn small_caps_and_small_cap_figures_extract_as_typed() {
    // Source Sans 3 substitutes letters and figures under `smcp` / `c2sc` (`one` → `one.s`),
    // in typed text, in a page number and in a list label alike (each with its own figures, so
    // one's mapping can't stand in for another's).
    use designcraft_doc::{Capitalization, ListType, story::PAGE_NUMBER};
    for caps in [Capitalization::SmallCaps, Capitalization::OpenTypeAllSmallCaps] {
        let text = format!("Abc 234 x{PAGE_NUMBER}\nItem");
        let mut d = doc_with_text(&text);
        let sid = *d.stories.keys().next().unwrap();
        let st = d.story_mut(sid).unwrap();
        st.format_paras(0..text.len(), |f| {
            f.chars.font_family = Some("Source Sans 3".into());
            f.chars.capitalization = Some(caps);
        });
        st.format_paras(text.len() - 1..text.len(), |f| {
            f.para.list_type = Some(ListType::Numbers);
            f.para.start_at = Some(Some(5));
        });
        let got = extract_text(&export_pdf(&d, &Cache::new(), &PdfOptions::default()).unwrap()).concat();
        assert!(!got.contains('\u{FFFD}'), "{caps:?}: {got:?}");
        assert_eq!(got.split_whitespace().collect::<Vec<_>>(), ["Abc", "234", "x15.Item"], "{caps:?}: {got:?}");
    }
    // All small caps draws `A` and `a` with one glyph, which one ToUnicode entry can't cover: the
    // second spelling goes out as ActualText (which this extractor doesn't read).
    let mut d = doc_with_text("Aa");
    let sid = *d.stories.keys().next().unwrap();
    d.story_mut(sid).unwrap().format_chars(0..2, |f| {
        f.over.font_family = Some("Source Sans 3".into());
        f.over.capitalization = Some(Capitalization::OpenTypeAllSmallCaps);
    });
    assert!(uncompressed(&d, PdfOptions::default()).contains("/ActualText"));
}

/// A clipping path on a placed image becomes a clip in the page content, inside the frame's.
#[test]
fn clipping_path_clips_the_image() {
    let png = |d: &mut Document| {
        let mut b = Vec::new();
        image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255]))
            .write_to(&mut std::io::Cursor::new(&mut b), image::ImageFormat::Png)
            .unwrap();
        place(d, b, (4, 4));
    };
    let clips = |s: &str| s.lines().filter(|l| matches!(l.trim(), "W" | "W*")).count();
    let mut d = doc_with_text("x");
    png(&mut d);
    let plain = uncompressed(&d, PdfOptions::default());
    let id = d.spreads[0].items.iter().find(|i| i.graphic().is_some()).unwrap().id;
    let triangle = designcraft_geom::PathData::single(designcraft_geom::path::SubPath::polyline(
        &[designcraft_geom::Point::new(0.0, 4.0), designcraft_geom::Point::new(2.0, 0.0), designcraft_geom::Point::new(4.0, 4.0)],
        true,
    ));
    let mut clip = designcraft_doc::ClippingPath::new(designcraft_doc::ClippingKind::DetectEdges, triangle);
    let set = |d: &mut Document, c: &designcraft_doc::ClippingPath| {
        if let Some(designcraft_doc::Content::Graphic(g)) = d.item_mut(id).map(|it| &mut it.content) {
            g.clip = Some(std::sync::Arc::new(c.clone()));
        }
    };
    set(&mut d, &clip);
    let clipped = uncompressed(&d, PdfOptions::default());
    assert_eq!(clips(&clipped), clips(&plain) + 1, "{clipped}");
    // The triangle's apex: graphic (2, 0) → page (172, 72) → PDF y = page height − 72.
    let apex_y = d.spreads[0].pages[0].height - 72.0;
    assert!(clipped.contains(&format!("172 {apex_y} l")), "{clipped}");
    clip.invert = true;
    set(&mut d, &clip);
    assert!(uncompressed(&d, PdfOptions::default()).lines().any(|l| l.trim() == "W*"), "an inverted clip is even-odd");
}

/// A page whose parent B is based on A carries A's items, then B's.
#[test]
fn based_on_parent_items_export_base_first() {
    let mut d = Document::new(&NewDocument { pages: 1, facing_pages: false, ..NewDocument::default() });
    d.swatches.push(Swatch::color("Base Cyan", Color::cmyk(1.0, 0.0, 0.0, 0.0)));
    d.swatches.push(Swatch::color("Parent Magenta", Color::cmyk(0.0, 1.0, 0.0, 0.0)));
    let a = d.parents[0].id;
    let b = d.add_parent("B", "Opening", 1, 12.0, designcraft_doc::Margins::uniform(36.0));
    std::sync::Arc::make_mut(&mut d.parents[1]).parent.as_mut().unwrap().based_on = Some(a);
    d.apply_parent(&[0], Some(b)).unwrap();
    let lid = d.default_layer();
    for (pi, swatch, r) in [(0, "Base Cyan", Rect::new(36.0, 36.0, 136.0, 136.0)), (1, "Parent Magenta", Rect::new(200.0, 36.0, 300.0, 136.0))] {
        let mut it = Item::new(ItemId(d.alloc()), lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(r));
        it.fill = Fill::swatch(swatch);
        d.insert_item(SpreadRef::Parent(pi), it, None).unwrap();
    }
    let s = uncompressed(&d, PdfOptions::default());
    let (cyan, magenta) = (s.find("1 0 0 0 k"), s.find("0 1 0 0 k"));
    assert!(cyan.is_some() && magenta.is_some(), "both parents' items are on the page");
    assert!(cyan < magenta, "the base parent's item is drawn first");
}

/// Form fields and buttons that don't print stay out of the PDF: inside a hidden group, on a
/// hidden or non-printing layer, or non-printing themselves (#726).
#[test]
fn only_printed_fields_and_buttons_export() {
    use designcraft_doc::{ButtonAction, Content, FieldKind, FormField, LayerId, Shape};
    use designcraft_geom::shapes::rectangle;
    fn control(d: &mut Document, layer: LayerId, y: f64, name: &str, button: bool) -> Item {
        let r = Rect::new(if button { 300.0 } else { 72.0 }, y, if button { 400.0 } else { 250.0 }, y + 24.0);
        let mut it = Item::new(ItemId(d.alloc()), layer, Shape::Rectangle, rectangle(r));
        if button {
            it.button = Some(ButtonAction::GoToUrl { url: format!("https://example.com/{name}") });
        } else {
            it.form_field = Some(FormField {
                kind: FieldKind::TextField,
                name: name.into(),
                value: String::new(),
                options: Vec::new(),
                multiline: false,
                required: false,
                font_size: 0.0,
            });
        }
        it
    }
    let export = |unprinted: bool| {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        for b in [false, true] {
            let it = control(&mut d, lid, 72.0, "shown", b);
            d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
        }
        if unprinted {
            let mut layers = Vec::new();
            for (visible, printable) in [(false, true), (true, false)] {
                let mut l = d.layers[0].clone();
                l.id = LayerId(d.alloc());
                l.visible = visible;
                l.printable = printable;
                d.layers.push(l.clone());
                layers.push(l.id);
            }
            for (i, layer) in layers.into_iter().enumerate() {
                for b in [false, true] {
                    let it = control(&mut d, layer, 120.0 + 48.0 * i as f64, &format!("unprinted-layer-{i}"), b);
                    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
                }
            }
            for b in [false, true] {
                let mut it = control(&mut d, lid, 240.0, "unprinted-item", b);
                it.nonprinting = true;
                d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
            }
            let children: Vec<_> = [false, true].map(|b| std::sync::Arc::new(control(&mut d, lid, 288.0, "unprinted-group", b))).into();
            let mut group = Item::new(ItemId(d.alloc()), lid, Shape::Group, rectangle(Rect::new(72.0, 288.0, 400.0, 312.0)));
            group.content = Content::Group { items: children };
            group.hidden = true;
            d.insert_item(SpreadRef::Doc(0), group, None).unwrap();
        }
        uncompressed(&d, PdfOptions::default())
    };
    let (shown, all) = (export(false), export(true));
    assert_eq!(all.matches("/Subtype/Widget").count(), 1, "one field");
    assert!(all.contains("(shown)") && !all.contains("unprinted"), "only the shown field and button");
    assert_eq!(all.matches("/Link").count(), shown.matches("/Link").count(), "only the shown button");
}

#[test]
fn form_field_border_follows_the_item_stroke() {
    use designcraft_doc::{FieldKind, FormField, Shape, Stroke};
    use designcraft_geom::shapes::rectangle;
    let export = |stroke: Stroke| {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let mut it = Item::new(ItemId(d.alloc()), lid, Shape::Rectangle, rectangle(Rect::new(72.0, 72.0, 250.0, 96.0)));
        it.stroke = stroke;
        it.form_field = Some(FormField {
            kind: FieldKind::TextField,
            name: "name".into(),
            value: String::new(),
            options: Vec::new(),
            multiline: false,
            required: false,
            font_size: 0.0,
        });
        d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
        uncompressed(&d, PdfOptions::default())
    };
    let none = export(Stroke { weight: 0.0, swatch: designcraft_color::swatch::NONE.into(), ..Stroke::default() });
    assert!(none.contains("/Subtype/Widget"));
    assert!(none.contains("/BS<</W 0>>"), "no border width");
    assert!(!none.contains("/BC"), "no border colour");
    let stroked = export(Stroke { weight: 1.0, ..Stroke::default() });
    assert!(stroked.contains("/BS<</W 1"), "border width");
    assert!(stroked.contains("/BC"), "border colour");
}

#[test]
fn extreme_page_selection_is_a_page_error_without_overflow() {
    let doc = doc_with_text("Bounded page selection");
    let opts = PdfOptions { pages: Some(vec![usize::MAX]), ..Default::default() };
    assert_eq!(export_pdf(&doc, &Cache::new(), &opts), Err(PdfError::BadPage(usize::MAX)));
}

#[test]
fn empty_and_populated_export_metadata_preserve_key_presence() {
    let mut d = Document::new(&NewDocument::default());
    d.title.clear();
    let s = uncompressed(&d, PdfOptions::default());
    for key in ["/Title", "/Author", "<dc:title>", "<dc:creator>"] {
        assert!(!s.contains(key), "unexpected empty metadata {key}");
    }
    d.title = "Synthetic title".into();
    let s = uncompressed(&d, PdfOptions { author: Some("Test author".into()), created: Some(1_700_000_000), ..Default::default() });
    for key in ["/Title", "/Author", "/CreationDate", "<dc:title>", "<dc:creator>"] {
        assert!(s.contains(key), "missing populated metadata {key}");
    }
    assert!(s.contains("Synthetic title") && s.contains("Test author"));
}

/// File › File Info goes into the PDF's document information and XMP; the export options' title
/// and author replace File Info's, and PDF/X-4 and Print Booklet keep them.
#[test]
fn file_info_is_written_into_the_pdf_metadata() {
    let mut d = doc_with_text("Body");
    d.title = "flyer-v3".into();
    d.file_info.document_title = "Spring Catalog".into();
    d.file_info.author = "Ana Ruiz".into();
    d.file_info.description = "Shoes for the spring season".into();
    d.file_info.keywords = vec!["spring".into(), "shoes".into()];
    let s = uncompressed(&d, PdfOptions::default());
    for want in ["/Title(Spring Catalog)", "/Author(Ana Ruiz)", "/Subject(Shoes for the spring season)", "/Keywords(spring, shoes)"] {
        assert!(s.contains(want), "missing {want}");
    }
    for want in ["<dc:title>", "<dc:creator>", "<dc:description>", "spring, shoes</pdf:Keywords>"] {
        assert!(s.contains(want), "XMP missing {want}");
    }
    assert!(!s.contains("flyer-v3"), "the document's name is not the title");
    let s = uncompressed(&d, PdfOptions { title: Some("Proof".into()), author: Some("Studio".into()), ..Default::default() });
    assert!(s.contains("/Title(Proof)") && s.contains("/Author(Studio)") && !s.contains("Ana Ruiz"), "the options win");
    let s = uncompressed(&d, PdfOptions { standard: Standard::PdfX4, ..Default::default() });
    assert!(s.contains("GTS_PDFXVersion"), "PDF/X-4");
    let booklet = export_booklet(&d, &Cache::new(), &BookletOptions::default()).unwrap();
    let b = String::from_utf8_lossy(&booklet.bytes);
    for (name, s) in [("PDF/X-4", s.as_str()), ("Print Booklet", &*b)] {
        for want in ["/Title(Spring Catalog)", "/Author(Ana Ruiz)", "/Subject(Shoes for the spring season)", "/Keywords(spring, shoes)"] {
            assert!(s.contains(want), "{name} missing {want}");
        }
        assert!(s.contains("<dc:description>") && s.contains("spring, shoes</pdf:Keywords>"), "{name} XMP");
    }
}

/// A swatch applied to a placed greyscale image colours it (#1094): the image's darkness is the
/// coverage of the swatch, so a spot swatch prints on its own plate. A colour image, or a swatch
/// that no longer exists, exports as the image itself.
#[test]
fn a_swatch_colours_a_greyscale_image() {
    let encode = |px: &[u8], ct: image::ExtendedColorType| {
        let mut b = Vec::new();
        image::ImageEncoder::write_image(image::codecs::png::PngEncoder::new(&mut b), px, 3, 1, ct).unwrap();
        b
    };
    let grey = encode(&[0, 64, 255], image::ExtendedColorType::L8);
    let rgb = encode(&[0, 0, 0, 200, 40, 40, 255, 255, 255], image::ExtendedColorType::Rgb8);
    let export = |file: &[u8], swatch: Option<&str>| {
        let mut d = doc_with_text("x");
        d.swatches.push(Swatch {
            name: "PANTONE Test".into(),
            value: SwatchValue::Color { color: Color::cmyk(0.0, 0.5, 1.0, 0.0), color_type: ColorType::Spot },
            locked: false,
            named: true,
            hidden: false,
        });
        place(&mut d, file.to_vec(), (3, 1));
        if let Some(sw) = swatch {
            let id = d.spreads[0].items.last().unwrap().id;
            if let designcraft_doc::Content::Graphic(g) = &mut d.item_mut(id).unwrap().content {
                g.fill = Fill::swatch(sw);
            }
        }
        let bytes = export_pdf(&d, &Cache::new(), &PdfOptions { compress: false, ..Default::default() }).unwrap();
        (String::from_utf8_lossy(&bytes).into_owned(), image_xobjects(&bytes))
    };
    let (plain, plain_images) = export(&grey, None);
    assert!(!plain.contains("PANTONE#20Test"), "the spot is used by nothing else");
    let (s, images) = export(&grey, Some("PANTONE Test"));
    assert!(s.contains("/Separation") && s.contains("/PANTONE#20Test"), "the image prints on the spot plate");
    // The swatch is painted through the image's coverage: black is full ink, white none.
    assert!(images.contains(&("DeviceGray".to_string(), vec![255, 191, 0])), "{images:?}");

    let (gone, gone_images) = export(&grey, Some("Gone"));
    assert_eq!(gone_images, plain_images, "a deleted swatch: the image as it is");
    assert!(!gone.contains("/SMask"), "nothing painted through it");
    let (s, _) = export(&rgb, Some("PANTONE Test"));
    assert!(!s.contains("PANTONE#20Test"), "a colour image keeps its colours");
}

/// An inline object is painted under its frame's text, as InDesign draws it: a picture whose
/// frame reaches down into the next line (a negative Y offset) doesn't hide that line.
#[test]
fn inline_objects_are_painted_under_the_text() {
    use designcraft_doc::{AnchorPosition, AnchoredObject};
    let mut d = doc_with_text("A\nWWWW");
    let sid = *d.stories.keys().next().unwrap();
    let mut item = Item::new(
        ItemId(d.alloc()),
        d.default_layer(),
        designcraft_doc::Shape::Rectangle,
        designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, 200.0, 30.0)),
    );
    item.fill = Fill::swatch(designcraft_color::swatch::BLACK);
    d.story_mut(sid).unwrap().insert_object(1, AnchoredObject::new(item, AnchorPosition::Inline { y_offset: -20.0 }));
    let order = paint_order(&export_pdf(&d, &Cache::new(), &PdfOptions::default()).unwrap());
    let first_text = order.iter().position(|(op, _)| op.ends_with("text")).unwrap();
    let the_box = order.iter().position(|(op, _)| op == "fill path").unwrap();
    assert!(the_box < first_text, "{order:?}");
}

/// An anchored object is painted as part of its host frame, as the screen renderer paints it:
/// inside the frame's transparency group (so a half-transparent frame shows its inline picture
/// half-transparent too), after the frame's fill and before its text and stroke.
#[test]
fn inline_objects_share_their_frames_transparency() {
    use designcraft_doc::{AnchorPosition, AnchoredObject};
    let mut d = doc_with_text("A\nWWWW");
    let sid = *d.stories.keys().next().unwrap();
    let fid = d.stories[&sid].frames[0];
    {
        let host = d.item_mut(fid).unwrap();
        host.opacity = 0.5;
        host.fill = Fill::swatch(designcraft_color::swatch::PAPER);
        host.stroke.weight = 3.0;
        host.stroke.swatch = designcraft_color::swatch::BLACK.into();
    }
    let mut item = Item::new(
        ItemId(d.alloc()),
        d.default_layer(),
        designcraft_doc::Shape::Rectangle,
        designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, 200.0, 30.0)),
    );
    item.fill = Fill::swatch(designcraft_color::swatch::BLACK);
    d.story_mut(sid).unwrap().insert_object(1, AnchoredObject::new(item, AnchorPosition::Inline { y_offset: -20.0 }));
    let ev = paint_events(&export_pdf(&d, &Cache::new(), &PdfOptions::default()).unwrap(), true);
    let at = |op: &str| ev.iter().position(|(o, _)| o == op).unwrap_or_else(|| panic!("no {op}: {ev:?}"));
    let group = ev.iter().position(|(o, a)| o == "group" && (*a - 0.5).abs() < 1e-3).unwrap_or_else(|| panic!("no half-opacity group: {ev:?}"));
    let end = group + ev[group..].iter().position(|(o, _)| o == "end group").unwrap();
    // Fills in order: the frame's, then the anchored box's.
    let fills: Vec<usize> = ev.iter().enumerate().filter(|(_, (o, _))| o == "fill path").map(|(i, _)| i).collect();
    assert!(fills.len() >= 2, "{ev:?}");
    let the_box = fills[1];
    assert!(group < the_box && the_box < end, "the box is inside the frame's group: {ev:?}");
    assert!(the_box < at("fill text") && at("fill text") < at("stroke path") && at("stroke path") < end, "{ev:?}");
}

/// The export asks for each story's composition once per page it is shown on: a parent page's
/// page number still reads its own page everywhere, and a story threaded across pages shows each
/// frame's own lines.
#[test]
fn each_page_shows_its_own_composition() {
    use designcraft_doc::story::PAGE_NUMBER;
    let mut d = Document::new(&NewDocument { pages: 3, facing_pages: false, ..NewDocument::default() });
    let lid = d.default_layer();
    d.add_text_frame(SpreadRef::Parent(0), Rect::new(36.0, 700.0, 300.0, 740.0), lid, &format!("Folio{PAGE_NUMBER}"), ParaFormat::default()).unwrap();
    let (first, _) =
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 56.0), lid, "Alpha\nBravo\nCharlie", ParaFormat::default()).unwrap();
    let mut prev = first;
    for si in 1..3 {
        let id = ItemId(d.alloc());
        let it = Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(36.0, 36.0, 300.0, 56.0)));
        d.insert_item(SpreadRef::Doc(si), it, None).unwrap();
        d.thread(prev, id).unwrap();
        prev = id;
    }
    let cache = Cache::new();
    let opts = PdfOptions { created: Some(0), ..PdfOptions::default() };
    let bytes = export_pdf(&d, &cache, &opts).unwrap();
    // Parent items first, then the page's own.
    assert_eq!(extract_text(&bytes), ["Folio1Alpha", "Folio2Bravo", "Folio3Charlie"]);
    assert_eq!(export_pdf(&d, &cache, &opts).unwrap(), bytes, "a warm cache exports the same bytes");
}

/// A parent page that overrode an item of its base and deleted the override hides that item on
/// the pages that use it. The canvas hid it but PDF export, which looked only at the page's own
/// overrides, still drew it (#1469).
#[test]
fn an_item_a_parent_overrode_and_deleted_is_not_exported() {
    let mut d = Document::new(&NewDocument { pages: 1, facing_pages: false, ..Default::default() });
    let lid = d.default_layer();
    let a = d.parents[0].id;
    let margins = d.parents[0].pages[0].margins;
    let b = d.add_parent("B", "Parent", 1, 0.0, margins);
    let box_id = ItemId(d.alloc());
    let rect = Rect::new(100.0, 100.0, 200.0, 160.0);
    let mut it = Item::new(box_id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(rect));
    it.fill = Fill::swatch(designcraft_color::swatch::BLACK);
    d.insert_item(SpreadRef::Parent(0), it, None).unwrap();
    std::sync::Arc::make_mut(&mut d.parents[1]).parent.as_mut().unwrap().based_on = Some(a);
    std::sync::Arc::make_mut(&mut d.spreads[0]).pages[0].parent = Some(b);
    let has_box = |d: &Document| {
        let (_, paths) = drawn(&export_pdf(d, &Cache::new(), &PdfOptions::default()).unwrap());
        paths.iter().any(|r| (r.width() - rect.width()).abs() < 0.5 && (r.height() - rect.height()).abs() < 0.5)
    };
    assert!(has_box(&d), "the page exports parent A's box through parent B");
    std::sync::Arc::make_mut(&mut d.parents[1]).pages[0].overridden.push(box_id);
    assert!(!has_box(&d), "removed on B, so the page using B exports no box");
}

/// Width, height and filter of each image XObject (soft masks aside).
fn image_dicts(bytes: &[u8]) -> Vec<(i32, i32, String)> {
    use hayro_syntax::object::Name;
    let pdf = hayro_syntax::Pdf::new(bytes.to_vec()).expect("parse");
    let mut out: Vec<(i32, i32, String)> = pdf
        .objects()
        .into_iter()
        .filter_map(|o| o.into_stream())
        .filter(|s| s.dict().get::<Name>("Subtype").is_some_and(|n| n.as_str() == "Image") && s.dict().get::<Name>("SMask").is_none())
        .map(|s| {
            let d = s.dict();
            (
                d.get::<i32>("Width").unwrap_or(-1),
                d.get::<i32>("Height").unwrap_or(-1),
                d.get::<Name>("Filter").map_or("none".into(), |n| n.as_str().to_string()),
            )
        })
        .collect();
    out.sort();
    out
}

fn compressed(d: &Document, images: ImageCompression) -> Vec<(i32, i32, String)> {
    image_dicts(&export_pdf(d, &Cache::new(), &PdfOptions { images, ..Default::default() }).unwrap())
}

fn colour(c: ImageClass) -> ImageCompression {
    ImageCompression { color: c, ..ImageCompression::NONE }
}

fn class(to: f64, above: f64, quality: Option<u8>) -> ImageClass {
    ImageClass { downsample_to: Some(to), above, quality }
}

fn grey_png(f: impl Fn(u32) -> u8) -> Vec<u8> {
    let img = image::GrayImage::from_fn(8, 8, |x, _| image::Luma([f(x)]));
    let mut b = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut b), image::ImageFormat::Png).unwrap();
    b
}

/// Compression: an image above its class's threshold is bicubic-downsampled to the class's
/// resolution; a JPEG quality makes DCT streams; a JPEG that isn't downsampled is passed through.
#[test]
fn compression_downsamples_per_class_and_sets_jpeg_quality() {
    // `place` scales 50×: 8 px over 400 pt = 1.44 ppi.
    let mut d = doc_with_text("x");
    place(&mut d, raster(image::ImageFormat::Png, false), (8, 8));
    // Above 1.2 ppi → down to 0.72 ppi = 4×4, lossless.
    assert_eq!(compressed(&d, colour(class(0.72, 1.2, None))), [(4, 4, "FlateDecode".into())]);
    // Under the threshold every pixel stays.
    assert_eq!(compressed(&d, colour(class(0.72, 2.0, None))), [(8, 8, "FlateDecode".into())]);
    // A quality makes a JPEG, downsampled or not.
    assert_eq!(compressed(&d, colour(class(0.72, 1.2, Some(60)))), [(4, 4, "DCTDecode".into())]);
    assert_eq!(compressed(&d, colour(ImageClass { quality: Some(60), ..ImageClass::LOSSLESS })), [(8, 8, "DCTDecode".into())]);
    assert_eq!(compressed(&d, ImageCompression::NONE), [(8, 8, "FlateDecode".into())]);

    // A placed JPEG is passed through unless downsampled.
    let mut j = doc_with_text("x");
    let bytes = raster(image::ImageFormat::Jpeg, false);
    place(&mut j, bytes.clone(), (8, 8));
    let pdf =
        export_pdf(&j, &Cache::new(), &PdfOptions { images: colour(ImageClass { quality: Some(20), ..ImageClass::LOSSLESS }), ..Default::default() })
            .unwrap();
    assert!(pdf.windows(bytes.len()).any(|w| w == bytes.as_slice()), "the JPEG's own bytes");
    assert_eq!(compressed(&j, colour(class(0.72, 1.2, Some(60)))), [(4, 4, "DCTDecode".into())]);

    // A CMYK TIFF is downsampled too and stays CMYK: lossless without a quality, a CMYK JPEG
    // with one.
    let (tif, _) = cmyk_tiff();
    let mut c = doc_with_text("x");
    place(&mut c, tif, (4, 2));
    let pdf = export_pdf(&c, &Cache::new(), &PdfOptions { images: colour(class(0.72, 1.2, None)), ..Default::default() }).unwrap();
    assert_eq!(image_dicts(&pdf), [(2, 1, "FlateDecode".into())]);
    assert_eq!(image_xobjects(&pdf)[0].0, "DeviceCMYK");
    let pdf = export_pdf(&c, &Cache::new(), &PdfOptions { images: colour(class(0.72, 1.2, Some(60))), ..Default::default() }).unwrap();
    assert_eq!(image_dicts(&pdf), [(2, 1, "DCTDecode".into())]);

    // A CMYK JPEG keeps its inks: passed through unless downsampled, and then a CMYK JPEG again.
    let cmyk = crate::image_fixtures::cmyk_jpeg([0, 255, 255, 255], Some(0));
    let mut k = doc_with_text("x");
    place(&mut k, cmyk.clone(), (8, 8));
    let pdf = export_pdf(&k, &Cache::new(), &PdfOptions { images: colour(class(0.72, 2.0, Some(60))), ..Default::default() }).unwrap();
    assert!(pdf.windows(cmyk.len()).any(|w| w == cmyk.as_slice()), "the CMYK JPEG's own bytes");
    let opts = PdfOptions { images: colour(class(0.72, 1.2, Some(60))), ..Default::default() };
    assert_eq!(image_dicts(&export_pdf(&k, &Cache::new(), &opts).unwrap()), [(4, 4, "DCTDecode".into())]);
    assert!(uncompressed(&k, opts).contains("/ColorSpace/DeviceCMYK"));
}

/// The grey class governs grey images; monochrome ones take the monochrome class and are never
/// JPEG.
#[test]
fn compression_grey_and_monochrome_classes() {
    let mut g = doc_with_text("x");
    place(&mut g, grey_png(|x| if x < 4 { 40 } else { 200 }), (8, 8));
    let images = ImageCompression { gray: class(0.72, 1.2, Some(60)), ..ImageCompression::NONE };
    assert_eq!(compressed(&g, images), [(4, 4, "DCTDecode".into())]);
    // The colour class leaves it alone.
    assert_eq!(compressed(&g, colour(class(0.72, 1.2, Some(60)))), [(8, 8, "FlateDecode".into())]);

    let mut m = doc_with_text("x");
    place(&mut m, grey_png(|x| if x < 4 { 0 } else { 255 }), (8, 8));
    let images = ImageCompression { gray: class(0.72, 1.2, Some(60)), mono: class(0.36, 1.2, Some(60)), ..ImageCompression::NONE };
    assert_eq!(compressed(&m, images), [(2, 2, "FlateDecode".into())]);
    // Monochrome follows its own class even when the grey one would downsample it.
    let images = ImageCompression { gray: class(0.72, 1.2, Some(60)), mono: ImageClass::LOSSLESS, ..ImageCompression::NONE };
    assert_eq!(compressed(&m, images), [(8, 8, "FlateDecode".into())]);

    // A grey JPEG the grey class doesn't downsample is decoded only to tell whether it is
    // monochrome (the monochrome class would downsample it); it is not, so it goes in as it is,
    // never a second lossy generation.
    let img = image::GrayImage::from_fn(8, 8, |x, y| image::Luma([(x * 30 + y * 3) as u8]));
    let mut jpg = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut jpg), image::ImageFormat::Jpeg).unwrap();
    let mut j = doc_with_text("x");
    place(&mut j, jpg.clone(), (8, 8));
    let images =
        ImageCompression { gray: ImageClass { quality: Some(20), ..ImageClass::LOSSLESS }, mono: class(0.36, 1.2, None), ..ImageCompression::NONE };
    let pdf = export_pdf(&j, &Cache::new(), &PdfOptions { images, ..Default::default() }).unwrap();
    assert!(pdf.windows(jpg.len()).any(|w| w == jpg.as_slice()), "the grey JPEG's own bytes");
}

/// The effective resolution follows the placed width: a graphic squeezed vertically only is not
/// downsampled more than its width says (the geometric mean of the scales would).
#[test]
fn compression_effective_resolution_follows_the_placed_width() {
    let mut d = doc_with_text("x");
    // 50× wide (1.44 ppi), 5× high: the mean scale (~15.8×) would make it 4.6 ppi.
    let xf = designcraft_geom::Affine::translate((72.0, 72.0)) * designcraft_geom::Affine::scale_non_uniform(50.0, 5.0);
    place_at(&mut d, raster(image::ImageFormat::Png, false), (8, 8), xf);
    assert_eq!(compressed(&d, colour(class(0.72, 2.0, None))), [(8, 8, "FlateDecode".into())]);
    assert_eq!(compressed(&d, colour(class(0.72, 1.2, None))), [(4, 4, "FlateDecode".into())]);
}

/// Two placements of one file share one embedded image, downsampled for the narrower one.
#[test]
fn compression_shared_file_uses_its_narrowest_placement() {
    let bytes = raster(image::ImageFormat::Png, false);
    for wide_first in [true, false] {
        let mut d = doc_with_text("x");
        // One of them 4× wider (0.36 ppi, never downsampled on its own).
        let wide = designcraft_geom::Affine::translate((72.0, 72.0)) * designcraft_geom::Affine::scale(200.0);
        let narrow = designcraft_geom::Affine::translate((72.0, 72.0)) * designcraft_geom::Affine::scale(50.0);
        let order = if wide_first { [wide, narrow] } else { [narrow, wide] };
        for xf in order {
            place_at(&mut d, bytes.clone(), (8, 8), xf);
        }
        assert_eq!(compressed(&d, colour(class(0.72, 1.2, None))), [(4, 4, "FlateDecode".into())], "wide first: {wide_first}");
    }
}

/// A placed PSD (read by `designcraft_images`, not by `image`) exports under Compression: as it
/// is, as JPEG, or downsampled, without warnings.
#[test]
fn psd_exports_through_compression() {
    let psd = {
        let mut b = Vec::new();
        b.extend(b"8BPS");
        b.extend(1u16.to_be_bytes());
        b.extend([0u8; 6]);
        b.extend(3u16.to_be_bytes()); // channels
        b.extend(1u32.to_be_bytes()); // height
        b.extend(2u32.to_be_bytes()); // width
        b.extend(8u16.to_be_bytes());
        b.extend(3u16.to_be_bytes()); // RGB
        b.extend([0u8; 12]); // colour mode data, image resources, layers
        b.extend(0u16.to_be_bytes()); // raw
        b.extend([255, 0, 0, 255, 0, 0]);
        b
    };
    let mut d = doc_with_text("x");
    place(&mut d, psd, (2, 1));
    let export = |color: ImageClass| {
        let r = export_pdf_with_report(&d, &Cache::new(), &PdfOptions { images: colour(color), ..Default::default() }).unwrap();
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        image_dicts(&r.bytes)
    };
    assert_eq!(export(ImageClass::LOSSLESS), [(2, 1, "FlateDecode".into())]);
    assert_eq!(export(ImageClass { quality: Some(60), ..ImageClass::LOSSLESS }), [(2, 1, "DCTDecode".into())]);
    // 2 px over 100 pt = 1.44 ppi; above 1.2 → down to 0.72 ppi = 1×1.
    assert_eq!(export(class(0.72, 1.2, None)), [(1, 1, "FlateDecode".into())]);
}

/// A minimal CMYK Photoshop file, 2×1: 100 % K, then 100 % C (channels stored inverted).
fn tiny_cmyk_psd() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(b"8BPS");
    b.extend(1u16.to_be_bytes());
    b.extend([0u8; 6]);
    b.extend(4u16.to_be_bytes()); // channels
    b.extend(1u32.to_be_bytes()); // height
    b.extend(2u32.to_be_bytes()); // width
    b.extend(8u16.to_be_bytes());
    b.extend(4u16.to_be_bytes()); // CMYK
    b.extend([0u8; 12]); // colour mode data, image resources, layers
    b.extend(0u16.to_be_bytes()); // raw
    b.extend([255, 0, 255, 255, 255, 255, 0, 255]); // C, M, Y, K planes
    b
}

/// A CMYK Photoshop file keeps its inks (100 % K stays K only) instead of going through RGB;
/// PDF/A gets it as RGB.
#[test]
fn cmyk_psd_keeps_its_inks() {
    let mut d = doc_with_text("x");
    place(&mut d, tiny_cmyk_psd(), (2, 1));
    let r = export_pdf_with_report(&d, &Cache::new(), &PdfOptions::default()).unwrap();
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(image_xobjects(&r.bytes), [("DeviceCMYK".to_string(), vec![0, 0, 0, 255, 255, 0, 0, 0])]);
    let r = export_pdf_with_report(&d, &Cache::new(), &PdfOptions { standard: Standard::PdfA2b, ..Default::default() }).unwrap();
    assert_ne!(image_xobjects(&r.bytes)[0].0, "DeviceCMYK");
}

fn convert(conversion: ColorConversion, destination: Option<&str>) -> Output {
    Output { conversion, destination: destination.map(str::to_string) }
}

/// Output › Convert to Destination: RGB swatches and RGB rasters become CMYK; CMYK and grey
/// keep their numbers in their own space; PDF/A stays RGB; an unknown destination is an error.
#[test]
fn color_conversion_to_destination_makes_cmyk() {
    let to_dest = convert(ColorConversion::ToDestination, None);
    let mut d = doc_with_text("x");
    d.swatches.push(Swatch::color("Web Red", Color::rgb(1.0, 0.0, 0.0)));
    add_box(&mut d, Rect::new(36.0, 36.0, 136.0, 136.0), "Web Red");
    add_box(&mut d, Rect::new(36.0, 200.0, 136.0, 300.0), "[Black]");
    let plain = uncompressed(&d, PdfOptions::default());
    assert!(plain.contains("1 0 0 rg"), "RGB stays RGB without conversion");
    let s = uncompressed(&d, PdfOptions { output: to_dest.clone(), ..Default::default() });
    // No RGB fill or stroke operator anywhere in the content.
    assert!(!s.split_ascii_whitespace().any(|t| t == "rg" || t == "RG"), "no RGB colour left:\n{}", &s[..s.len().min(2500)]);
    assert!(s.contains("0 0 0 1 k"), "black keeps its numbers");

    // An RGB PNG becomes a DeviceCMYK image; a grey one stays grey.
    let mut d = doc_with_text("x");
    place(&mut d, raster(image::ImageFormat::Png, false), (8, 8));
    let r = export_pdf_with_report(&d, &Cache::new(), &PdfOptions { output: to_dest.clone(), ..Default::default() }).unwrap();
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(image_xobjects(&r.bytes)[0].0, "DeviceCMYK");
    let a2b = PdfOptions { output: to_dest.clone(), standard: Standard::PdfA2b, ..Default::default() };
    let r = export_pdf_with_report(&d, &Cache::new(), &a2b).unwrap();
    assert_ne!(image_xobjects(&r.bytes)[0].0, "DeviceCMYK", "PDF/A stays RGB");
    assert!(r.warnings.iter().any(|w| w.contains("PDF/A: Output colour conversion was left out")), "{:?}", r.warnings);
    // A placed RGB JPEG is converted too, not passed through.
    let mut j = doc_with_text("x");
    place(&mut j, raster(image::ImageFormat::Jpeg, false), (8, 8));
    let r = export_pdf_with_report(&j, &Cache::new(), &PdfOptions { output: to_dest.clone(), ..Default::default() }).unwrap();
    assert_eq!(image_xobjects(&r.bytes)[0].0, "DeviceCMYK");
    let mut g = doc_with_text("x");
    place(&mut g, grey_png(|x| if x < 4 { 40 } else { 200 }), (8, 8));
    let r = export_pdf_with_report(&g, &Cache::new(), &PdfOptions { output: to_dest, ..Default::default() }).unwrap();
    assert_eq!(image_xobjects(&r.bytes)[0].0, "DeviceGray");

    let bad = convert(ColorConversion::PreserveNumbers, Some("No Such Profile"));
    assert!(export_pdf(&g, &Cache::new(), &PdfOptions { output: bad, ..Default::default() }).is_err());
}

/// Into another destination than the working CMYK, Convert to Destination moves CMYK colours
/// and CMYK images; Preserve Numbers keeps them. The PDF/X-4 output intent names the destination.
#[test]
fn convert_and_preserve_numbers_differ_and_the_output_intent_is_the_destination() {
    let other = designcraft_color::cms::GENERIC_CMYK;
    let mut d = doc_with_text("x");
    d.swatches.push(Swatch::color("Ink", Color::cmyk(0.6, 0.3, 0.1, 0.05)));
    add_box(&mut d, Rect::new(36.0, 36.0, 136.0, 136.0), "Ink");
    let (tif, inks) = cmyk_tiff();
    place(&mut d, tif, (4, 2));
    let export = |conversion| {
        let opts = PdfOptions { output: convert(conversion, Some(other)), compress: false, ..Default::default() };
        let bytes = export_pdf(&d, &Cache::new(), &opts).unwrap();
        (String::from_utf8_lossy(&bytes).into_owned(), image_xobjects(&bytes))
    };
    let (plain, plain_images) = export(ColorConversion::None);
    let (kept, kept_images) = export(ColorConversion::PreserveNumbers);
    let (moved, moved_images) = export(ColorConversion::ToDestination);
    let fill = |s: &str| s.lines().find(|l| l.ends_with(" k")).map(str::to_string);
    assert_eq!(fill(&kept), fill(&plain), "Preserve Numbers keeps the swatch's numbers");
    assert_ne!(fill(&moved), fill(&plain), "Convert to Destination converts them");
    assert_eq!(plain_images[0].1, inks);
    assert_eq!(kept_images[0], ("DeviceCMYK".to_string(), inks.clone()));
    assert_eq!(moved_images[0].0, "DeviceCMYK");
    assert_ne!(moved_images[0].1, inks);

    // PDF/X-4: the output intent is the destination when converting, else the working CMYK.
    let x4 = |output: Output| {
        let bytes = export_pdf(&d, &Cache::new(), &PdfOptions { standard: Standard::PdfX4, output, ..Default::default() }).unwrap();
        crate::print_tests::output_intent(&bytes).expect("output intent")
    };
    let names = |i: &crate::print_tests::Intent| format!("{} / {}", i.identifier, i.condition);
    let (dest, working) = (x4(convert(ColorConversion::PreserveNumbers, Some(other))), x4(Output::default()));
    assert!(names(&dest).contains("Generic CMYK"), "{}", names(&dest));
    assert!(!names(&working).contains("Generic CMYK"), "{}", names(&working));
    assert_ne!(dest.profile, working.profile, "the destination's own profile");
}

/// With an Image Quality, CMYK rasters go in as CMYK JPEG (Adobe-inverted, with a Decode array):
/// a CMYK TIFF, and an RGB raster converted to the destination. Without one they stay lossless.
#[test]
fn cmyk_rasters_with_a_quality_are_cmyk_jpeg() {
    let (tif, _) = cmyk_tiff();
    let mut c = doc_with_text("x");
    place(&mut c, tif, (4, 2));
    let lossless = uncompressed(&c, PdfOptions { images: colour(class(0.72, 1.2, None)), ..Default::default() });
    assert!(lossless.contains("/ColorSpace/DeviceCMYK") && !lossless.contains("/DCTDecode"));
    let cmyk_jpeg = |s: &str| s.contains("/Filter/DCTDecode") && s.contains("/ColorSpace/DeviceCMYK") && s.contains("/Decode[1 0 1 0 1 0 1 0]");
    let s = uncompressed(&c, PdfOptions { images: colour(class(0.72, 1.2, Some(60))), ..Default::default() });
    assert!(cmyk_jpeg(&s), "{}", &s[..s.len().min(3000)]);
    // The legacy switch alone never makes a CMYK raster JPEG.
    assert!(!uncompressed(&c, PdfOptions { compress_images: true, ..Default::default() }).contains("/DCTDecode"));

    let mut d = doc_with_text("x");
    place(&mut d, raster(image::ImageFormat::Png, false), (8, 8));
    let opts = PdfOptions {
        output: convert(ColorConversion::ToDestination, None),
        images: colour(ImageClass { quality: Some(70), ..ImageClass::LOSSLESS }),
        ..Default::default()
    };
    let s = uncompressed(&d, opts);
    assert!(cmyk_jpeg(&s), "{}", &s[..s.len().min(3000)]);
    // Converted, the legacy switch alone doesn't make it JPEG either.
    let s = uncompressed(&d, PdfOptions { output: convert(ColorConversion::ToDestination, None), compress_images: true, ..Default::default() });
    assert!(s.contains("/ColorSpace/DeviceCMYK") && !s.contains("/DCTDecode"), "{}", &s[..s.len().min(3000)]);
}

/// PDF/A takes no CMYK: a placed CMYK JPEG goes in as RGB, downsampled or not, never as the
/// DeviceCMYK JPEG it is.
#[test]
fn pdf_a_writes_a_cmyk_jpeg_as_rgb() {
    let cmyk = crate::image_fixtures::cmyk_jpeg([0, 255, 255, 255], Some(0));
    let mut k = doc_with_text("x");
    place(&mut k, cmyk, (8, 8));
    for images in [ImageCompression::NONE, colour(class(0.72, 1.2, Some(60)))] {
        let s = uncompressed(&k, PdfOptions { standard: Standard::PdfA2b, images, ..Default::default() });
        assert!(s.contains("/Subtype/Image") && !s.contains("DeviceCMYK"), "{images:?}: {}", &s[..s.len().min(3000)]);
    }
}
