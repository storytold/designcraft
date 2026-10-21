//! Placed files: every placement of one file shares an asset.

use std::cell::Cell;

use designcraft_doc::{AssetId, Content, PdfCrop};

use super::*;

const PHOTO: &str = "file:/photos/wedding%20day/couple.jpg";

/// A graphic frame at `x` showing `image` (an `<Image>` element) with `crop` on every side.
fn frame(n: usize, x: f64, crop: f64, image: &str) -> String {
    format!(
        r#"<Rectangle Self="r{n}" ItemLayer="L1" ItemTransform="1 0 0 1 {x} 0" ContentType="GraphicType">
      <Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
        <PathPointType Anchor="0 0"/><PathPointType Anchor="0 30"/><PathPointType Anchor="40 30"/><PathPointType Anchor="40 0"/>
      </PathPointArray></GeometryPathType></PathGeometry></Properties>
      <FrameFittingOption TopCrop="{crop}" LeftCrop="{crop}" BottomCrop="{crop}" RightCrop="{crop}"/>
      {image}
    </Rectangle>"#
    )
}

fn image(n: usize, uri: &str, contents: Option<&[u8]>) -> String {
    let contents = contents.map(|c| format!("<Contents><![CDATA[{}]]></Contents>", base64_encode(c))).unwrap_or_default();
    format!(
        r#"<Image Self="im{n}" ItemTransform="1 0 0 1 {n} 0">
        <Properties>{contents}<GraphicBounds Left="0" Top="0" Right="40" Bottom="30"/></Properties>
        <Link Self="lk{n}" LinkResourceURI="{uri}" LinkResourceFormat="$ID/JPEG" StoredState="Normal"/>
      </Image>"#
    )
}

/// A graphic frame showing `insert.pdf` (a `<PDF>` element) at page `PageNumber` (1-based),
/// cropped to `crop` (`PDFCrop`).
fn pdf_frame(n: usize, x: f64, page: u32, crop: &str) -> String {
    format!(
        r#"<Rectangle Self="r{n}" ItemLayer="L1" ItemTransform="1 0 0 1 {x} 0" ContentType="GraphicType">
      <Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
        <PathPointType Anchor="0 0"/><PathPointType Anchor="0 30"/><PathPointType Anchor="40 30"/><PathPointType Anchor="40 0"/>
      </PathPointArray></GeometryPathType></PathGeometry></Properties>
      <PDF Self="im{n}" ItemTransform="1 0 0 1 {n} 0">
        <Properties><GraphicBounds Left="0" Top="0" Right="40" Bottom="30"/></Properties>
        <Link Self="lk{n}" LinkResourceURI="file:/art/insert.pdf" LinkResourceFormat="$ID/PDF" StoredState="Normal"/>
        <PDFAttribute PageNumber="{page}" PDFCrop="{crop}" TransparentBackground="true"/>
      </PDF>
    </Rectangle>"#
    )
}

fn package(frames: &[String]) -> Vec<u8> {
    let spread = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <Spread Self="sp1" PageCount="1" ItemTransform="1 0 0 1 0 0">
    <Page Self="p1" AppliedMaster="m1" GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 0 0"/>
    {}
  </Spread>
</idPkg:Spread>"#,
        frames.join("\n")
    );
    zip_files(&[
        ("designmap.xml", DESIGNMAP),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Styles.xml", STYLES),
        ("Resources/Preferences.xml", PREFS),
        ("MasterSpreads/MasterSpread_m1.xml", MASTER),
        ("Spreads/Spread_sp1.xml", &spread),
        ("Stories/Story_s1.xml", STORY),
    ])
}

/// (asset, crop, transform) of each graphic, left to right.
fn placements(d: &Document) -> Vec<(AssetId, [f64; 4], designcraft_geom::Affine)> {
    let mut out: Vec<(f64, AssetId, [f64; 4], designcraft_geom::Affine)> = d.spreads[0]
        .items
        .iter()
        .filter_map(|i| match &i.content {
            Content::Graphic(g) => Some((i.xf.translation().x, g.asset, g.crop, g.xf)),
            _ => None,
        })
        .collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out.into_iter().map(|(_, a, c, x)| (a, c, x)).collect()
}

#[test]
fn one_file_placed_several_times_imports_as_one_asset() {
    let bytes = vec![42u8; 300];
    let reads = Cell::new(0);
    let reader = |p: &str| {
        reads.set(reads.get() + 1);
        p.ends_with("couple.jpg").then(|| bytes.clone())
    };
    // Three placements linking the file, and one embedding the same bytes under another path:
    // that one keeps its own asset and path.
    let frames = vec![
        frame(1, 0.0, 1.0, &image(1, PHOTO, None)),
        frame(2, 100.0, 2.0, &image(2, PHOTO, None)),
        frame(3, 200.0, 3.0, &image(3, PHOTO, None)),
        frame(4, 300.0, 4.0, &image(4, "file:/elsewhere/copy.jpg", Some(&bytes))),
    ];
    let d = import_idml_with(&package(&frames), &reader).unwrap();
    assert_eq!(reads.get(), 1, "the linked file is read once");
    assert_eq!(d.assets.len(), 2);
    let p = placements(&d);
    assert_eq!(p.iter().map(|(_, c, _)| c[0]).collect::<Vec<_>>(), vec![1.0, 2.0, 3.0, 4.0]);
    let link = |id| d.assets.get(id).and_then(|a| a.link.as_deref());
    let links: Vec<_> = p.iter().map(|(id, _, _)| link(id)).collect();
    assert_eq!(links[..3], [Some("/photos/wedding day/couple.jpg"); 3]);
    assert_eq!(links[3], Some("/elsewhere/copy.jpg"));
    assert!(p[..3].iter().all(|(id, _, _)| *id == p[0].0));

    // Exported linked or embedded, the round trip keeps the two assets and each placement's crop
    // and transform.
    for embed_images in [false, true] {
        let out = export_idml_with(&d, &ExportOptions { embed_images });
        let back = import_idml_with(&out, &reader).unwrap();
        assert_eq!(back.assets.len(), 2, "embed_images: {embed_images}");
        let q = placements(&back);
        assert_eq!(q.len(), 4);
        for ((_, c0, x0), (_, c1, x1)) in p.iter().zip(&q) {
            assert_eq!(c0, c1);
            assert!(x0.as_coeffs().iter().zip(x1.as_coeffs()).all(|(a, b)| (a - b).abs() < 1e-6), "{x0:?} {x1:?}");
        }
    }
}

/// An `<SVG>` placement linking `uri`, whose GraphicBounds are `bounds` (left top right bottom).
fn svg(n: usize, uri: &str, bounds: [f64; 4]) -> String {
    let [l, t, r, b] = bounds;
    format!(
        r#"<SVG Self="sv{n}" ItemTransform="1 0 0 1 0 0">
        <Properties><GraphicBounds Left="{l}" Top="{t}" Right="{r}" Bottom="{b}"/></Properties>
        <Link Self="lk{n}" LinkResourceURI="{uri}" LinkResourceFormat="$ID/SVG" StoredState="Normal"/>
      </SVG>"#
    )
}

#[test]
fn placed_svg_with_only_a_viewbox_keeps_its_viewbox_size() {
    const OVERHANG: &str =
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100"><line x1="-10" y1="50" x2="0" y2="50" stroke="#000"/></svg>"##;
    const SIZED: &str =
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 200 100"><rect width="200" height="100"/></svg>"#;
    let reader = |p: &str| match p {
        "/art/overhang.svg" => Some(OVERHANG.as_bytes().to_vec()),
        "/art/sized.svg" => Some(SIZED.as_bytes().to_vec()),
        _ => None,
    };
    // GraphicBounds are the art's bounds: 211 wide for the overhanging one, placed twice (the second
    // reuses the first's asset); a sized SVG is still stretched onto its bounds.
    let frames = vec![
        frame(1, 0.0, 0.0, &svg(1, "file:/art/overhang.svg", [-11.0, 0.0, 200.0, 100.0])),
        frame(2, 100.0, 0.0, &svg(2, "file:/art/overhang.svg", [-11.0, 0.0, 200.0, 100.0])),
        frame(3, 200.0, 0.0, &svg(3, "file:/art/sized.svg", [-10.0, 0.0, 200.0, 100.0])),
    ];
    let d = import_idml_with(&package(&frames), &reader).unwrap();
    let mut gs: Vec<_> = d.spreads[0]
        .items
        .iter()
        .filter_map(|i| match &i.content {
            Content::Graphic(g) => Some((i.xf.translation().x, g.size, g.xf.translation().x)),
            _ => None,
        })
        .collect();
    gs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let got: Vec<_> = gs.into_iter().map(|(_, size, x)| (size, x)).collect();
    assert_eq!(got, vec![((200.0, 100.0), -11.0), ((200.0, 100.0), -11.0), ((210.0, 100.0), -10.0)]);
}

/// `PDFAttribute/@PageNumber` (1-based) is read into `Asset.page` (0-based), and the same file at
/// different pages stays apart (#905).
#[test]
fn a_pdf_placement_reads_its_page_number() {
    let bytes = b"%PDF-1.7\n% fixture".to_vec();
    let reader = |p: &str| p.ends_with("insert.pdf").then(|| bytes.clone());
    let frames = vec![pdf_frame(1, 0.0, 2, "CropMedia"), pdf_frame(2, 100.0, 1, "CropMedia"), pdf_frame(3, 200.0, 2, "CropMedia")];
    let d = import_idml_with(&package(&frames), &reader).unwrap();
    let p = placements(&d);
    let page = |id| d.assets.get(&id).map(|a| a.page);
    assert_eq!(page(p[0].0), Some(1), "PageNumber=\"2\" is the second page, stored 0-based");
    assert_eq!(page(p[1].0), Some(0), "PageNumber=\"1\" is the first page");
    assert_eq!(p[0].0, p[2].0, "the same file at the same page shares one asset");
    assert_ne!(p[0].0, p[1].0, "the same file at another page is another asset");

    // Exported (linked or embedded), every placement writes its page and crop, page 1 included,
    // and both survive the round trip.
    for embed_images in [false, true] {
        let out = export_idml_with(&d, &ExportOptions { embed_images });
        let spreads = super::zip_text(&out, "Spreads/");
        assert_eq!(spreads.matches(r#"PDFCrop="CropMedia""#).count(), 3, "{spreads}");
        assert!(spreads.contains(r#"PageNumber="1""#) && spreads.contains(r#"PageNumber="2""#), "{spreads}");
        let back = import_idml_with(&out, &reader).unwrap();
        let shown: Vec<_> = placements(&back).iter().map(|(id, _, _)| back.assets.get(id).map(|a| (a.page, a.pdf_crop))).collect();
        let (first, second) = (Some((0, PdfCrop::Media)), Some((1, PdfCrop::Media)));
        assert_eq!(shown, vec![second, first, second], "embed_images: {embed_images}");
    }
}

/// InDesign embeds an EPS placement's preview as `EPSImage` + five words + raw top-down RGB.
fn eps_preview(w: u32, h: u32, rgb: &[u8]) -> Vec<u8> {
    let mut b = b"EPSImage".to_vec();
    for v in [w * 3, w, h, 8, 3] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(rgb);
    b
}

#[test]
fn an_embedded_eps_preview_imports_as_a_raster() {
    // 2x2: red, green / blue, white.
    let rgb = [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
    let frames = vec![frame(1, 0.0, 0.0, &image(1, "file:/art/code.eps", Some(&eps_preview(2, 2, &rgb))))];
    let d = import_idml_with(&package(&frames), &|_: &str| None).unwrap();
    let (id, _, _) = placements(&d)[0];
    let a = d.assets.get(&id).unwrap();
    assert_eq!(a.mime, "image/bmp");
    assert_eq!(a.pixels, Some((2, 2)));
    assert!(a.is_raster());
    // A 24-bit BMP stores rows bottom-up as BGR, padded to four bytes: blue/white first.
    let px = &a.data[54..];
    assert_eq!(px[..6], [255, 0, 0, 255, 255, 255], "bottom row: blue, white");
    assert_eq!(px[8..14], [0, 0, 255, 0, 255, 0], "top row: red, green");

    // A truncated raster is not decoded as one.
    let cut = eps_preview(2, 2, &rgb[..9]);
    assert!(crate::eps_preview_to_bmp(&cut).is_none());
}

/// `EPSTextData` as InDesign writes it: header, font record, five doubles, text record.
fn eps_text_data(font: &str, size: f64, x: f64, y: f64, text: &str) -> Vec<u8> {
    let n = text.chars().count() as u8;
    let mut tail = vec![n, 0x80, n];
    for u in text.encode_utf16() {
        tail.extend_from_slice(&u.to_be_bytes());
    }
    eps_text_record(font, size, x, y, &tail)
}

/// The same with the text record (its length, then its runs) given as bytes.
fn eps_text_record(font: &str, size: f64, x: f64, y: f64, text: &[u8]) -> Vec<u8> {
    let mut b = vec![0, 1, 0, 2, 2, 1, 1, 0, font.len() as u8, 0x40, font.len() as u8];
    b.extend_from_slice(font.as_bytes());
    for v in [size, 0.0, 0.0, size, x, y, 0.0, 0.0] {
        b.extend_from_slice(&v.to_be_bytes());
    }
    b.extend_from_slice(&[2, 1, 1, 0]);
    b.extend_from_slice(text);
    b.extend_from_slice(&[0; 40]);
    b
}

#[test]
fn eps_text_runs_of_8_bit_and_utf16_characters_join() {
    let parse = |text: &[u8]| crate::parse_eps_text(&eps_text_record("YuMincho-Regular", 21.0, 1.0, 2.0, text));
    // 7 characters: a wide run of five, then a narrow one of two.
    let text = [7, 0x80, 5, 0x91, 0xce, 0x57, 0x30, 0x54, 0x08, 0x67, 0x7f, 0xff, 0x20, 0x40, 2, b'1', b'2'];
    assert_eq!(parse(&text).unwrap().text, "野地合板＠12");
    // 6 characters: a narrow run of two, then a wide one of four.
    let mut text = vec![6, 0x40, 2, b'G', b'L', 0x80, 4];
    for u in "鋼板です".encode_utf16() {
        text.extend_from_slice(&u.to_be_bytes());
    }
    assert_eq!(parse(&text).unwrap().text, "GL鋼板です");
    // A run list that stops short of its length is not text.
    assert!(parse(&[5, 0x40, 2, b'a', b'b']).is_none());
}

#[test]
fn eps_text_reads_font_size_position_and_text() {
    let t = crate::parse_eps_text(&eps_text_data("YuMincho-Regular", 21.0, 377.5, -793.25, "ご予約ページ")).unwrap();
    assert_eq!((t.font.as_str(), t.size, t.x, t.y, t.text.as_str()), ("YuMincho-Regular", 21.0, 377.5, -793.25, "ご予約ページ"));
    assert!(crate::parse_eps_text(b"EPSImage").is_none(), "not an EPSText record");
}

#[test]
fn a_placed_eps_text_imports_as_a_text_frame() {
    let data = base64_encode(&eps_text_data("YuMincho-Regular", 20.0, 100.0, -50.0, "0120-672-334"));
    // The art spans 100..220 in its own coordinates and was scaled by one half in the layout.
    let eps = format!(
        r#"<EPSText Self="e1" FillColor="Color/Black" ItemTransform="1 0 0 1 0 0"><Properties>
        <PathBoundingBox Left="300" Top="-70" Right="360" Bottom="-60"/>
        <EPSTextData><![CDATA[{data}]]></EPSTextData>
        <EPSTextAttributeBounds Top="-65" Left="100" Bottom="-45" Right="220"/></Properties></EPSText>"#
    );
    let group = format!(r#"<Group Self="g1" ItemLayer="L1" ItemTransform="1 0 0 1 0 0">{eps}</Group>"#);
    let d = import_idml_with(&package(&[group]), &|_: &str| None).unwrap();
    let Content::Group { items } = &d.spreads[0].items[0].content else { panic!("a group") };
    let Content::Text(tf) = &items[0].content else { panic!("the EPS text is a text frame") };
    let story = d.story(tf.story).unwrap();
    assert_eq!(story.text, "0120-672-334");
    let run = &story.chars[0].format.over;
    assert_eq!((run.font_family.as_deref(), run.font_style.as_deref()), (Some("Yu Mincho"), Some("Regular")));
    assert_eq!(run.size, Some(10.0), "20 pt at the layout's scale of one half");
    // The baseline stays where the EPS put it: 5 pt above the art's bottom (-45), which the layout
    // scaled by one half, so 2.5 pt above the bottom of the bounds (-60).
    let top = items[0].path.bounds().unwrap().y0;
    assert!((top + tf.options.first_baseline_min - -62.5).abs() < 1e-6, "baseline {}", top + tf.options.first_baseline_min);
    assert_eq!(story.frames, vec![items[0].id]);
}

/// Two crops of one linked PDF page are two assets, so each frame keeps its own crop; the same
/// page and crop placed again shares an asset, and the file is read once per page and crop.
#[test]
fn a_pdf_page_placed_with_two_crops_imports_as_two_assets() {
    let bytes = b"%PDF-1.7\n% fixture".to_vec();
    let reads = Cell::new(0);
    let reader = |p: &str| {
        reads.set(reads.get() + 1);
        p.ends_with("insert.pdf").then(|| bytes.clone())
    };
    let frames = vec![pdf_frame(1, 0.0, 1, "CropTrim"), pdf_frame(2, 100.0, 1, "CropMedia"), pdf_frame(3, 200.0, 1, "CropTrim")];
    let d = import_idml_with(&package(&frames), &reader).unwrap();
    let p = placements(&d);
    let crop = |id| d.assets.get(&id).map(|a| (a.page, a.pdf_crop));
    assert_eq!(crop(p[0].0), Some((0, PdfCrop::Trim)));
    assert_eq!(crop(p[1].0), Some((0, PdfCrop::Media)));
    assert_ne!(p[0].0, p[1].0, "another crop of the same page is another asset");
    assert_eq!(p[0].0, p[2].0, "the same page and crop share one asset");
    assert_eq!(d.assets.len(), 2);
    assert_eq!(reads.get(), 2);
}

/// The swatch applied to a placed image (`<Image FillColor FillTint>`) colours a greyscale or
/// bitmap image in InDesign (#1094). Black at 100%, InDesign's own value for an image nobody
/// coloured, and None leave the image as it is, like a missing attribute.
#[test]
fn the_swatch_applied_to_an_image_is_kept() {
    let bytes = vec![42u8; 300];
    let reader = |p: &str| p.ends_with("couple.jpg").then(|| bytes.clone());
    let coloured = |n: usize, attrs: &str| image(n, PHOTO, None).replacen(&format!(r#"Self="im{n}""#), &format!(r#"Self="im{n}" {attrs}"#), 1);
    let frames = vec![
        frame(1, 0.0, 0.0, &coloured(1, r#"FillColor="Color/Brand" FillTint="50""#)),
        frame(2, 100.0, 0.0, &coloured(2, r#"FillColor="Color/Black" FillTint="-1""#)),
        frame(3, 200.0, 0.0, &coloured(3, r#"FillColor="Swatch/None""#)),
        frame(4, 300.0, 0.0, &image(4, PHOTO, None)),
        frame(5, 400.0, 0.0, &coloured(5, r#"FillColor="Color/u9""#)),
        frame(6, 500.0, 0.0, &coloured(6, r#"FillColor="Color/Black" FillTint="40""#)),
    ];
    let fills = |d: &Document| {
        let mut v: Vec<(f64, (String, f32))> = d.spreads[0]
            .items
            .iter()
            .filter_map(|i| match &i.content {
                Content::Graphic(g) => Some((i.xf.translation().x, (g.fill.swatch.clone(), g.fill.tint))),
                _ => None,
            })
            .collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        v.into_iter().map(|(_, f)| f).collect::<Vec<_>>()
    };
    let none = || (designcraft_color::swatch::NONE.to_string(), 1.0);
    let want = vec![
        ("Brand".to_string(), 0.5),
        none(),
        none(),
        none(),
        ("C=10 M=20 Y=30 K=40".to_string(), 1.0),
        (designcraft_color::swatch::BLACK.to_string(), 0.4),
    ];
    let d = import_idml_with(&package(&frames), &reader).unwrap();
    assert_eq!(fills(&d), want);

    let out = export_idml_with(&d, &ExportOptions::default());
    let spreads = zip_text(&out, "Spreads/");
    let images: Vec<&str> = spreads.split("<Image ").skip(1).map(|s| s.split('>').next().unwrap_or("")).collect();
    assert_eq!(images.len(), 6);
    // Only an image with a swatch says so; none of them gets an invented `Swatch/None`.
    assert_eq!(images.iter().filter(|e| e.contains("FillColor=")).count(), 3, "{images:?}");
    assert!(images.iter().any(|e| e.contains(r#"FillColor="Color/Brand""#) && e.contains(r#"FillTint="50""#)), "{images:?}");
    assert!(!images.iter().any(|e| e.contains("Swatch/None")), "{images:?}");
    assert_eq!(fills(&import_idml_with(&out, &reader).unwrap()), want);
}

/// One `EMR_STRETCHDIBITS` record: a `depth`-bit bitmap with an optional colour `table`.
fn emf_dib_record(w: u32, h: u32, depth: u16, table: &[u8], bits: &[u8]) -> Vec<u8> {
    let le = |v: u32| v.to_le_bytes();
    let mut bmi = [le(40), le(w), le(h)].concat();
    bmi.extend(1u16.to_le_bytes());
    bmi.extend(depth.to_le_bytes());
    bmi.extend([le(0), le(bits.len() as u32)].concat());
    bmi.resize(40, 0);
    bmi.extend(table);
    let mut rec = [le(81), le((80 + bmi.len() + bits.len()) as u32)].concat();
    rec.resize(48, 0);
    rec.extend([le(80), le(bmi.len() as u32), le(80 + bmi.len() as u32), le(bits.len() as u32)].concat());
    rec.resize(80, 0);
    rec.extend(&bmi);
    rec.extend(bits);
    rec
}

/// An EMF the way InDesign embeds a pasted picture: header, the given records, `EMR_EOF`.
fn emf_of(records: &[Vec<u8>]) -> Vec<u8> {
    let le = |v: u32| v.to_le_bytes();
    let mut b = [le(1), le(88)].concat();
    b.resize(40, 0);
    b.extend(b" EMF");
    b.resize(88, 0);
    for r in records {
        b.extend(r);
    }
    b.extend([le(14), le(20), le(0), le(16), le(20)].concat());
    b
}

/// An EMF holding one 24-bit bitmap.
fn emf_with_bitmap(w: u32, h: u32, bgr: &[u8]) -> Vec<u8> {
    emf_of(&[emf_dib_record(w, h, 24, &[], bgr)])
}

#[test]
fn a_placed_emf_photo_imports_as_a_raster() {
    // 2x1, 24-bit rows padded to four bytes: blue, green.
    let bgr = [255, 0, 0, 0, 255, 0, 0, 0];
    let emf = emf_with_bitmap(2, 1, &bgr);
    let bmp = crate::emf_raster_to_bmp(&emf).unwrap();
    assert_eq!(&bmp[..2], b"BM");
    assert_eq!(bmp[10..14], 54u32.to_le_bytes());
    assert_eq!(bmp[54..], bgr);
    let frames = vec![frame(1, 0.0, 0.0, &image(1, "file:/art/photo.emf", Some(&emf)))];
    let d = import_idml_with(&package(&frames), &|_: &str| None).unwrap();
    let (id, _, _) = placements(&d)[0];
    let a = d.assets.get(&id).unwrap();
    assert_eq!(a.mime, "image/bmp");
    assert_eq!(a.pixels, Some((2, 1)));

    // Not an EMF, or an EMF with no bitmap in it: nothing to show.
    assert!(crate::emf_raster_to_bmp(b"not an emf").is_none());
    assert!(crate::emf_raster_to_bmp(&emf[..88]).is_none());
}

#[test]
fn a_16_bit_emf_bitmap_is_widened_to_24_bit() {
    // 2x1 at 5-5-5: pure red (0x7c00) and pure blue (0x001f), rows padded to four bytes.
    let emf = emf_of(&[emf_dib_record(2, 1, 16, &[], &[0x00, 0x7c, 0x1f, 0x00])]);
    let bmp = crate::emf_raster_to_bmp(&emf).unwrap();
    assert_eq!(bmp[28..30], 24u16.to_le_bytes());
    assert_eq!(bmp[54..60], [0, 0, 255, 255, 0, 0], "BGR: red, blue");
}

#[test]
fn a_masked_emf_shows_its_colour_bitmap_with_the_mask_as_alpha() {
    // InDesign writes a transparent picture as a 1-bit mask record (black, white), then the
    // colour record; the picture shows where the mask is white.
    let table = [0, 0, 0, 0, 255, 255, 255, 0];
    let mask = emf_dib_record(2, 1, 1, &table, &[0b0100_0000, 0, 0, 0]);
    let colour = emf_dib_record(2, 1, 24, &[], &[10, 20, 30, 40, 50, 60, 0, 0]);
    let bmp = crate::emf_raster_to_bmp(&emf_of(&[mask, colour])).unwrap();
    assert_eq!(bmp[28..30], 32u16.to_le_bytes());
    assert_eq!(bmp[10..14], 122u32.to_le_bytes());
    assert_eq!(bmp[122..], [10, 20, 30, 0, 40, 50, 60, 255], "the first pixel is outside the mask");
}
