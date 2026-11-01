use craft_pdf::write::backend as krilla;
use std::sync::Arc;

use designcraft_color::{Color, Swatch};
use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Asset, AssetId, Content, Document, Fill, Fitting, Graphic, Item, ItemId, Shape, SpreadRef};
use designcraft_geom::{Affine, Rect};
use hayro_syntax::object::{Name, Object};
use image::{ExtendedColorType, ImageEncoder};

use crate::{ExportReport, PdfOptions, Standard, check_pdfx4, export_pdf_with_report};

pub(super) fn place(d: &mut Document, name: &str, data: Vec<u8>, size: (f64, f64), at: (f64, f64)) {
    let asset = AssetId(d.alloc());
    let mime = designcraft_images::mime(&data).into();
    let pixels = designcraft_images::pixel_size(&data);
    d.assets.insert(asset, Arc::new(Asset { id: asset, name: name.into(), mime, data: data.into(), pixels, ..Default::default() }));
    let lid = d.default_layer();
    let id = ItemId(d.alloc());
    let r = Rect::new(at.0, at.1, at.0 + 100.0, at.1 + 50.0);
    let mut it = Item::new(id, lid, Shape::Rectangle, designcraft_geom::shapes::rectangle(r));
    it.content = Content::Graphic(Graphic {
        layer_options: None,
        asset,
        size,
        xf: Affine::translate(at) * Affine::scale(100.0 / size.0),
        clip: None,
        auto_fit: Fitting::FillProportionally,
        fit_align: 4,
        crop: [0.0; 4],
        wrap: Default::default(),
        fill: Default::default(),
    });
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
}

fn add_box(d: &mut Document, r: Rect, swatch: &str) {
    let lid = d.default_layer();
    let mut b = Item::new(ItemId(d.alloc()), lid, Shape::Rectangle, designcraft_geom::shapes::rectangle(r));
    b.fill = Fill::swatch(swatch);
    d.insert_item(SpreadRef::Doc(0), b, None).unwrap();
}

/// A 4 × 2 PNG: red RGB or mid grey, optionally carrying an ICC profile.
fn png(gray: bool, icc: Option<Vec<u8>>) -> Vec<u8> {
    let mut b = Vec::new();
    let mut enc = image::codecs::png::PngEncoder::new(&mut b);
    if let Some(p) = icc {
        enc.set_icc_profile(p).unwrap();
    }
    let (px, ty) = if gray { (vec![128u8; 8], ExtendedColorType::L8) } else { ([255u8, 0, 0].repeat(8), ExtendedColorType::Rgb8) };
    enc.write_image(&px, 4, 2, ty).unwrap();
    b
}

/// The header of an ICC v2.1 RGB display profile: what a PDF writer reads to tag an image
/// `/ICCBased` (the check looks at the colour space, not at the profile's tables).
fn rgb_icc() -> Vec<u8> {
    let mut p = vec![0u8; 132];
    p[..4].copy_from_slice(&132u32.to_be_bytes());
    p[8] = 2;
    p[9] = 0x10;
    p[12..16].copy_from_slice(b"mntr");
    p[16..20].copy_from_slice(b"RGB ");
    p[20..24].copy_from_slice(b"XYZ ");
    p[36..40].copy_from_slice(b"acsp");
    p
}

/// A 4 × 2 CMYK image (100% K) as krilla writes it.
#[derive(Clone, Hash)]
struct CmykImage(Vec<u8>);

impl krilla::image::CustomImage for CmykImage {
    fn color_channel(&self) -> &[u8] {
        &self.0
    }
    fn alpha_channel(&self) -> Option<&[u8]> {
        None
    }
    fn bits_per_component(&self) -> krilla::image::BitsPerComponent {
        krilla::image::BitsPerComponent::Eight
    }
    fn size(&self) -> (u32, u32) {
        (4, 2)
    }
    fn icc_profile(&self) -> Option<&[u8]> {
        None
    }
    fn color_space(&self) -> krilla::image::ImageColorspace {
        krilla::image::ImageColorspace::Cmyk
    }
}

/// A one-page PDF 1.6 holding a CMYK image, as another app would hand over a placed graphic.
fn pdf_with_cmyk_image() -> Vec<u8> {
    pdf_with_image(krilla::image::Image::from_custom(CmykImage([0u8, 0, 0, 255].repeat(8)), false).unwrap())
}

/// A one-page PDF 1.6 holding `img`.
fn pdf_with_image(img: krilla::image::Image) -> Vec<u8> {
    let configuration = krilla::configure::ConfigurationBuilder::new().with_version(krilla::configure::PdfVersion::Pdf16).finish().unwrap();
    let mut pdf = krilla::Document::new_with(krilla::SerializeSettings { configuration, ..Default::default() });
    let size = krilla::geom::Size::from_wh(100.0, 50.0).unwrap();
    let mut page = pdf.start_page_with(krilla::page::PageSettings::new(size));
    let mut s = page.surface();
    s.draw_image(img, size);
    s.finish();
    page.finish();
    pdf.finish().unwrap()
}

pub(super) fn x4(d: &Document) -> ExportReport {
    export_pdf_with_report(d, &Cache::new(), &PdfOptions { standard: Standard::PdfX4, ..Default::default() }).unwrap()
}

/// The `/ColorSpace` of every image XObject: a name, or `[` for an array (ICCBased, Indexed…).
fn image_spaces(bytes: &[u8]) -> Vec<String> {
    let pdf = hayro_syntax::Pdf::new(bytes.to_vec()).unwrap();
    let mut out: Vec<String> = pdf
        .objects()
        .into_iter()
        .filter_map(|o| o.into_stream())
        .filter(|s| s.dict().get::<Name>(b"Subtype").is_some_and(|n| n.as_str() == "Image") && s.dict().get::<bool>(b"ImageMask").is_none())
        .filter_map(|s| match s.dict().get::<Object>(b"ColorSpace")? {
            Object::Name(n) => Some(n.as_str().to_string()),
            Object::Array(a) => a.iter::<Name>().next().map(|n| format!("[{}", n.as_str())),
            _ => None,
        })
        .collect();
    out.sort();
    out
}

/// A DeviceRGB image under the CMYK output intent is reported. A placed PDF carries its
/// images into the export unchanged.
#[test]
fn device_rgb_image_is_flagged() {
    let mut d = Document::new(&NewDocument::default());
    let photo = krilla::image::Image::from_png(png(false, None).into(), false).unwrap();
    place(&mut d, "photo.pdf", pdf_with_image(photo), (100.0, 50.0), (72.0, 72.0));
    let r = x4(&d);
    assert_eq!(image_spaces(&r.bytes), ["DeviceRGB"]);
    let rgb: Vec<&String> = r.warnings.iter().filter(|w| w.contains("RGB")).collect();
    assert_eq!(rgb.len(), 1, "{:?}", r.warnings);
    assert!(rgb[0].starts_with("PDF/X-4: an RGB image (object ") && rgb[0].contains("4 × 2 px) on page 1:"), "{}", rgb[0]);
    // Without a PDF/X output intent there is nothing to check RGB against.
    let plain = export_pdf_with_report(&d, &Cache::new(), &PdfOptions::default()).unwrap();
    assert!(!check_pdfx4(&plain.bytes).iter().any(|i| i.contains("RGB")));
}

/// CMYK and grey are the output intent's own values and ICC-tagged RGB is colour-managed:
/// none of them is flagged, whether placed directly or inside a placed PDF.
#[test]
fn cmyk_grey_and_icc_rgb_pass() {
    let mut d = Document::new(&NewDocument::default());
    add_box(&mut d, Rect::new(36.0, 400.0, 136.0, 500.0), "C=100 M=0 Y=0 K=0");
    place(&mut d, "grey.png", png(true, None), (4.0, 2.0), (72.0, 72.0));
    place(&mut d, "tagged.png", png(false, Some(rgb_icc())), (4.0, 2.0), (72.0, 172.0));
    place(&mut d, "logo.pdf", pdf_with_cmyk_image(), (100.0, 50.0), (72.0, 272.0));
    let r = x4(&d);
    assert_eq!(image_spaces(&r.bytes), ["DeviceCMYK", "DeviceGray", "[ICCBased"]);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
}

/// RGB swatches paint in DeviceRGB.
#[test]
fn device_rgb_fills_are_flagged() {
    let mut d = Document::new(&NewDocument { pages: 2, ..NewDocument::default() });
    d.swatches.push(Swatch::color("Brand Red", Color::rgb(0.9, 0.1, 0.1)));
    add_box(&mut d, Rect::new(36.0, 400.0, 136.0, 500.0), "Brand Red");
    let r = x4(&d);
    assert_eq!(r.warnings, ["PDF/X-4: RGB fills or strokes on page 1: under a CMYK output intent RGB needs an ICC profile"]);
}

/// A minimal PDF with a CMYK (N 4) PDF/X output intent and one page with `entries` (its
/// resources, a group…) and `content`.
fn tiny_pdf(entries: &str, content: &str) -> Vec<u8> {
    tiny_pdf_with(entries, content, "<</N 3/Length 0>>\nstream\n\nendstream")
}

/// [`tiny_pdf`] with `extra` as object 6 (an RGB profile in [`tiny_pdf`]).
pub(super) fn tiny_pdf_with(entries: &str, content: &str, extra: &str) -> Vec<u8> {
    let objects = [
        "<</Type/Catalog/Pages 2 0 R/OutputIntents[<</Type/OutputIntent/S/GTS_PDFX/DestOutputProfile 4 0 R>>]>>".to_string(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
        format!("<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]{entries}/Contents 5 0 R>>"),
        "<</N 4/Length 0>>\nstream\n\nendstream".to_string(),
        format!("<</Length {}>>\nstream\n{content}\nendstream", content.len()),
        extra.to_string(),
    ];
    let mut out = b"%PDF-1.6\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f\r\n", objects.len() + 1).as_bytes());
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<</Size {}/Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
    out
}

/// A metadata object whose only `stream` is the one inside `endstream` is refused, not a slice panic.
#[test]
fn metadata_with_only_endstream_does_not_panic() {
    let objects = ["<</Type/Catalog/Pages 2 0 R/Metadata 3 0 R>>", "<</Type/Pages/Kids[]/Count 0>>", "<</Type/Metadata>>endstream"];
    let mut out = b"%PDF-1.6\n".to_vec();
    for (i, o) in objects.iter().enumerate() {
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    out.extend_from_slice(b"trailer\n<</Size 4/Root 1 0 R>>\nstartxref\n0\n%%EOF\n");
    assert!(super::make_pdfx4(&out, None).is_none());
}

/// DeviceRGB is allowed when a `DefaultRGB` colour space makes it colour-managed.
#[test]
fn default_rgb_makes_device_rgb_managed() {
    let rgb = |i: &String| i.contains("RGB");
    let content = "1 0 0 rg 0 0 10 10 re f /DeviceRGB CS 0 0 1 SC 0 0 m 10 10 l S";
    let bare = check_pdfx4(&tiny_pdf("/Resources<<>>", content));
    assert_eq!(bare.iter().filter(|i| rgb(i)).count(), 1, "{bare:?}");
    let managed = check_pdfx4(&tiny_pdf("/Resources<</ColorSpace<</DefaultRGB[/ICCBased 6 0 R]>>>>", content));
    assert!(!managed.iter().any(rgb), "{managed:?}");
    // CMYK and grey need no default space.
    let cmyk = check_pdfx4(&tiny_pdf("/Resources<<>>", "0 0 0 1 k 0 0 10 10 re f 0.5 G 0 0 m 10 10 l S"));
    assert!(!cmyk.iter().any(rgb), "{cmyk:?}");
}

/// Every font that draws visible text must be embedded (#909): a base-14 font with no font
/// file, a CID font whose descendant has none, and one drawn from inside a form are reported;
/// an embedded font, an unused one and invisible text (render mode 3, as in OCR layers) are not.
#[test]
fn unembedded_fonts_are_flagged() {
    let fonts = |entries: &str, content: &str| -> Vec<String> {
        check_pdfx4(&tiny_pdf(entries, content)).into_iter().filter(|i| i.contains("font")).collect()
    };
    let helvetica = "/Resources<</Font<</F1<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>>>>>";
    assert_eq!(
        fonts(helvetica, "BT /F1 24 Tf 20 50 Td (Base14 text) Tj ET"),
        ["the font Helvetica isn't embedded (page 1): PDF/X-4 needs every font used for visible text embedded"]
    );
    assert!(fonts(helvetica, "BT /F1 24 Tf 3 Tr 20 50 Td (OCR) Tj ET").is_empty(), "invisible text needs no embedded font");
    assert!(fonts(helvetica, "0 0 0 1 k 0 0 10 10 re f").is_empty(), "an unused font is not reported");
    let restored = fonts(helvetica, "BT /F1 24 Tf q 3 Tr Q 20 50 Td (shown) Tj ET");
    assert_eq!(restored.len(), 1, "Q restores the visible render mode: {restored:?}");

    let embedded = "/Resources<</Font<</F1<</Type/Font/Subtype/TrueType/BaseFont/ABCDEF+Arial/FontDescriptor<</Type/FontDescriptor/FontName/ABCDEF+Arial/FontFile2 6 0 R>>>>>>>>";
    assert!(fonts(embedded, "BT /F1 12 Tf (x) Tj ET").is_empty());
    let cid = "/Resources<</Font<</F1<</Type/Font/Subtype/Type0/BaseFont/KozMin/Encoding/Identity-H/DescendantFonts[<</Type/Font/Subtype/CIDFontType0/BaseFont/KozMin/FontDescriptor<</Type/FontDescriptor/FontName/KozMin>>>>]>>>>>>";
    assert_eq!(fonts(cid, "BT /F1 12 Tf <0001> Tj ET").len(), 1);
    let cid_embedded = cid.replace("/FontName/KozMin>>", "/FontName/KozMin/FontFile3 6 0 R>>");
    assert!(fonts(&cid_embedded, "BT /F1 12 Tf <0001> Tj ET").is_empty());

    let draw = "BT /T 9 Tf (in a form) Tj ET";
    let form = format!(
        "<</Type/XObject/Subtype/Form/BBox[0 0 100 100]/Resources<</Font<</T<</Type/Font/Subtype/Type1/BaseFont/Times-Roman>>>>>>/Length {}>>\nstream\n{draw}\nendstream",
        draw.len()
    );
    let pdf = tiny_pdf_with("/Resources<</XObject<</X1 6 0 R>>>>", "/X1 Do", &form);
    let issues: Vec<String> = check_pdfx4(&pdf).into_iter().filter(|i| i.contains("font")).collect();
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert!(issues[0].starts_with("the font Times-Roman isn't embedded"), "{issues:?}");
}

/// A placed PDF that draws with an unembedded font: the PDF/X-4 export warns about it (#909).
#[test]
fn placed_pdf_with_an_unembedded_font_is_reported() {
    let mut d = Document::new(&NewDocument::default());
    let placed = tiny_pdf(
        "/Resources<</Font<</F1<</Type/Font/Subtype/Type1/BaseFont/Helvetica/Encoding/WinAnsiEncoding>>>>>>",
        "BT /F1 24 Tf 0 0 0 1 k 20 50 Td (Base14 text) Tj ET",
    );
    place(&mut d, "helvetica.pdf", placed, (100.0, 100.0), (72.0, 72.0));
    let r = x4(&d);
    assert!(r.warnings.iter().any(|w| w.starts_with("PDF/X-4: the font Helvetica isn't embedded (page 1)")), "{:?}", r.warnings);
}

#[test]
fn unembedded_fonts_are_reported_by_every_text_show_operator() {
    let entries = "/Resources<</Font<</F1<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>>>>>";
    for show in ["(x) Tj", "[(x) 20 (y)] TJ", "(x) '", "0 0 (x) \""] {
        let pdf = tiny_pdf(entries, &format!("BT /F1 12 Tf {show} ET"));
        let issues = check_pdfx4(&pdf);
        assert_eq!(issues.iter().filter(|i| i.contains("isn't embedded")).count(), 1, "{show}: {issues:?}");
    }
}

#[test]
fn unembedded_fonts_in_forms_inherit_and_restore_text_state() {
    let entries = "/Resources<</Font<</F1<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>>>/XObject<</X1 6 0 R>>>>";
    let draw = "BT (inherited font) Tj /F1 10 Tf 3 Tr ET";
    let form = format!("<</Type/XObject/Subtype/Form/BBox[0 0 100 100]/Length {}>>\nstream\n{draw}\nendstream", draw.len());
    for (content, count) in [
        ("/F1 12 Tf /X1 Do", 1),
        ("/F1 12 Tf 3 Tr /X1 Do", 0),
        ("/F1 12 Tf 3 Tr /X1 Do 0 Tr /X1 Do", 1),
        ("/F1 12 Tf /X1 Do BT (still visible) Tj ET", 1),
    ] {
        let pdf = tiny_pdf_with(entries, content, &form);
        let issues = check_pdfx4(&pdf);
        assert_eq!(issues.iter().filter(|i| i.contains("isn't embedded")).count(), count, "{content}: {issues:?}");
    }
}

#[test]
fn unembedded_fonts_in_recursive_forms_are_bounded() {
    let draw = "BT /F1 12 Tf (x) Tj ET /X1 Do";
    let form = format!(
        "<</Type/XObject/Subtype/Form/BBox[0 0 100 100]/Resources<</Font<</F1<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>>>/XObject<</X1 6 0 R>>>>/Length {}>>\nstream\n{draw}\nendstream",
        draw.len()
    );
    let pdf = tiny_pdf_with("/Resources<</XObject<</X1 6 0 R>>>>", "/X1 Do", &form);
    let issues = check_pdfx4(&pdf);
    assert_eq!(issues.iter().filter(|i| i.contains("isn't embedded")).count(), 1, "{issues:?}");
}

#[test]
fn unembedded_fonts_are_reported_after_form_text_state_is_restored() {
    let entries = "/Resources<</Font<</F1<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>>>/XObject<</X1 6 0 R>>>>";
    let draw = "BT /F2 10 Tf 3 Tr (hidden) Tj ET";
    let form = format!(
        "<</Type/XObject/Subtype/Form/BBox[0 0 100 100]/Resources<</Font<</F2<</Type/Font/Subtype/Type1/BaseFont/Times-Roman>>>>>>/Length {}>>\nstream\n{draw}\nendstream",
        draw.len()
    );
    let pdf = tiny_pdf_with(entries, "/F1 12 Tf /X1 Do BT (visible) Tj ET", &form);
    let issues: Vec<String> = check_pdfx4(&pdf).into_iter().filter(|i| i.contains("isn't embedded")).collect();
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert!(issues[0].starts_with("the font Helvetica isn't embedded"), "{issues:?}");
}

/// An RGB blending space on the page group and an RGB inline image are device RGB too.
#[test]
fn rgb_groups_and_inline_images_are_flagged() {
    let group = check_pdfx4(&tiny_pdf("/Resources<<>>/Group<</S/Transparency/CS/DeviceRGB>>", "0 0 0 1 k"));
    assert!(
        group.iter().any(|i| i == "a transparency group blending in RGB on page 1: under a CMYK output intent RGB needs an ICC profile"),
        "{group:?}"
    );
    let inline = check_pdfx4(&tiny_pdf("/Resources<<>>", "q BI /W 1 /H 1 /CS /RGB /BPC 8 /F /AHx ID ff0000> EI Q"));
    assert!(inline.iter().any(|i| i.starts_with("an RGB inline image on page 1")), "{inline:?}");
    let grey = check_pdfx4(&tiny_pdf("/Resources<<>>", "q BI /W 1 /H 1 /CS /G /BPC 8 /F /AHx ID 80> EI Q"));
    assert!(!grey.iter().any(|i| i.contains("RGB")), "{grey:?}");
}

/// A 4 × 2 RGB image in every raster path the exporter has, none with a profile. Each has
/// its own colour, so no two share an image XObject.
fn rgb_rasters() -> Vec<(&'static str, Vec<u8>)> {
    let px = |c: [u8; 3]| c.repeat(8);
    let encode = |f: &dyn Fn(&mut Vec<u8>)| {
        let mut b = Vec::new();
        f(&mut b);
        b
    };
    let jpeg = encode(&|b| image::codecs::jpeg::JpegEncoder::new(b).write_image(&px([0, 0, 255]), 4, 2, ExtendedColorType::Rgb8).unwrap());
    let gif = encode(&|b| image::codecs::gif::GifEncoder::new(b).encode(&[0u8, 255, 0, 255].repeat(8), 4, 2, ExtendedColorType::Rgba8).unwrap());
    let webp = encode(&|b| image::codecs::webp::WebPEncoder::new_lossless(b).write_image(&px([255, 255, 0]), 4, 2, ExtendedColorType::Rgb8).unwrap());
    let tiff = encode(&|b| {
        image::codecs::tiff::TiffEncoder::new(std::io::Cursor::new(b)).write_image(&px([0, 255, 255]), 4, 2, ExtendedColorType::Rgb8).unwrap()
    });
    vec![("photo.png", png(false, None)), ("photo.jpg", jpeg), ("photo.gif", gif), ("photo.webp", webp), ("photo.tif", tiff)]
}

/// The decoded `/ICCBased` profile of every image XObject that has one.
fn image_profiles(bytes: &[u8]) -> Vec<Vec<u8>> {
    let pdf = hayro_syntax::Pdf::new(bytes.to_vec()).unwrap();
    pdf.objects()
        .into_iter()
        .filter_map(|o| o.into_stream())
        .filter(|s| s.dict().get::<Name>(b"Subtype").is_some_and(|n| n.as_str() == "Image"))
        .filter_map(|s| match s.dict().get::<hayro_syntax::object::Array>(b"ColorSpace")?.iter::<Object>().nth(1)? {
            Object::Stream(p) => Some(p.decoded().ok()?.into_owned()),
            _ => None,
        })
        .collect()
}

/// PDF/X-4 wrote placed RGB images as DeviceRGB under the CMYK output intent, and only warned.
/// Each raster path now tags them with sRGB, re-encoded as JPEG or not. A plain export keeps
/// DeviceRGB.
#[test]
fn rgb_images_carry_srgb_in_pdfx4() {
    let mut d = Document::new(&NewDocument::default());
    for (i, (name, data)) in rgb_rasters().into_iter().enumerate() {
        place(&mut d, name, data, (4.0, 2.0), (72.0, 72.0 + 60.0 * i as f64));
    }
    for compress_images in [false, true] {
        let opts = PdfOptions { standard: Standard::PdfX4, compress_images, ..Default::default() };
        let r = export_pdf_with_report(&d, &Cache::new(), &opts).unwrap();
        assert!(r.warnings.is_empty(), "compress_images {compress_images}: {:?}", r.warnings);
        assert_eq!(image_spaces(&r.bytes), ["[ICCBased"; 5], "compress_images {compress_images}");
        let profiles = image_profiles(&r.bytes);
        assert_eq!(profiles.len(), 5);
        assert!(profiles.iter().all(|p| p == crate::export::SRGB_ICC), "every image carries the sRGB profile");
    }
    let plain = export_pdf_with_report(&d, &Cache::new(), &PdfOptions::default()).unwrap();
    assert_eq!(image_spaces(&plain.bytes), ["DeviceRGB"; 5]);
}

/// An image's own profile and a grey image are left as they are in PDF/X-4.
#[test]
fn own_profiles_and_grey_stay() {
    let mut d = Document::new(&NewDocument::default());
    place(&mut d, "grey.png", png(true, None), (4.0, 2.0), (72.0, 72.0));
    place(&mut d, "tagged.png", png(false, Some(rgb_icc())), (4.0, 2.0), (72.0, 172.0));
    let r = x4(&d);
    assert_eq!(image_spaces(&r.bytes), ["DeviceGray", "[ICCBased"]);
    assert_eq!(image_profiles(&r.bytes), [rgb_icc()]);
}

/// Every image constructor asked for interpolation, which PDF/X-4 and PDF/A forbid: PDF/A
/// exports with a placed image failed (#71). Plain exports keep it.
#[test]
fn no_interpolation_in_pdfx4_or_pdfa() {
    let mut d = Document::new(&NewDocument::default());
    for (i, (name, data)) in rgb_rasters().into_iter().enumerate() {
        place(&mut d, name, data, (4.0, 2.0), (72.0, 72.0 + 60.0 * i as f64));
    }
    let interpolated = |b: &[u8]| String::from_utf8_lossy(b).matches("/Interpolate true").count();
    for compress_images in [false, true] {
        for standard in [Standard::PdfX4, Standard::PdfA2b] {
            let opts = PdfOptions { standard, compress_images, ..Default::default() };
            let r = export_pdf_with_report(&d, &Cache::new(), &opts).unwrap_or_else(|e| panic!("{standard:?}: {e:?}"));
            assert_eq!(interpolated(&r.bytes), 0, "{standard:?}, compress_images {compress_images}");
        }
        let plain = export_pdf_with_report(&d, &Cache::new(), &PdfOptions { compress_images, ..Default::default() }).unwrap();
        // The TIFF goes through the decoded fallback, which never asked for interpolation,
        // unless it is re-encoded as JPEG.
        assert_eq!(interpolated(&plain.bytes), if compress_images { 5 } else { 4 }, "compress_images {compress_images}");
        assert!(check_pdfx4(&plain.bytes).iter().any(|i| i == "an image asks for interpolation"));
    }
    let tif = cmyk_tiff();
    let mut c = Document::new(&NewDocument::default());
    place(&mut c, "scan.tif", tif, (4.0, 2.0), (72.0, 72.0));
    assert_eq!(interpolated(&x4(&c).bytes), 0, "CMYK TIFF");
}

/// A 4 × 2 CMYK TIFF, 100% K.
fn cmyk_tiff() -> Vec<u8> {
    let mut b = Vec::new();
    tiff::encoder::TiffEncoder::new(std::io::Cursor::new(&mut b))
        .unwrap()
        .write_image::<tiff::encoder::colortype::CMYK8>(4, 2, &[0u8, 0, 0, 255].repeat(8))
        .unwrap();
    b
}

/// The XMP states the trapping state as the document information does.
#[test]
fn xmp_states_trapped() {
    let r = x4(&Document::new(&NewDocument::default()));
    let pdf = hayro_syntax::Pdf::new(r.bytes.clone()).unwrap();
    let xref = pdf.xref();
    let catalog = xref.get::<hayro_syntax::object::Dict>(xref.root_id()).unwrap();
    let xmp = catalog.get::<hayro_syntax::object::Stream>(b"Metadata").unwrap().decoded().unwrap().into_owned();
    let xmp = String::from_utf8_lossy(&xmp);
    assert!(xmp.contains("xmlns:pdf=\"http://ns.adobe.com/pdf/1.3/\"") && xmp.contains("<pdf:Trapped>False</pdf:Trapped>"), "{xmp}");
    assert!(String::from_utf8_lossy(&r.bytes).contains("/Trapped/False"));
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
}
