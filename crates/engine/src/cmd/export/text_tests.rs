use serde_json::json;

use crate::Session;

fn unzip(b64: &str) -> zip::ZipArchive<std::io::Cursor<Vec<u8>>> {
    zip::ZipArchive::new(std::io::Cursor::new(super::super::file::base64_decode(b64))).unwrap()
}

fn read(z: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>, name: &str) -> String {
    let mut s = String::new();
    std::io::Read::read_to_string(&mut z.by_name(name).unwrap(), &mut s).unwrap();
    s
}

/// Split Document is stored with the document as one undo step, follows a renamed style, and
/// an export can override it without storing its own.
#[test]
fn epub_split_document_setting() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "One\nText\nTwo\nMore"})).unwrap();
    let sid = r["story"].as_u64().unwrap();
    s.execute("style.paragraph.create", &json!({"name": "Chapter"})).unwrap();
    for at in [0, 9] {
        s.execute("text.select", &json!({"story": sid, "anchor": at, "focus": at})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name": "Chapter"})).unwrap();
    }
    let files = |s: &mut Session, p: serde_json::Value| {
        let mut z = unzip(s.execute("file.exportEpub", &p).unwrap()["base64"].as_str().unwrap());
        (1..).take_while(|n| z.by_name(&if *n == 1 { "OEBPS/content.xhtml".into() } else { format!("OEBPS/content{n}.xhtml") }).is_ok()).count()
    };
    assert_eq!(s.execute("document.epubSplit", &json!({})).unwrap()["split"], "off");
    assert_eq!(files(&mut s, json!({})), 1, "one content file by default");
    assert!(s.execute("document.epubSplit", &json!({"split": {"style": "Nope"}})).is_err());
    assert!(s.execute("document.epubSplit", &json!({"split": "h1"})).is_err());
    assert!(s.execute("file.exportEpub", &json!({"split": {"style": "Nope"}})).is_err());
    assert_eq!(s.execute("document.epubSplit", &json!({"split": {"style": "Chapter"}})).unwrap()["split"], json!({"style": "Chapter"}));
    assert_eq!(files(&mut s, json!({})), 2);
    assert_eq!(files(&mut s, json!({"split": "off"})), 1);
    assert_eq!(s.doc().unwrap().doc.settings.epub_split, designcraft_doc::EpubSplit::Style("Chapter".into()), "the override isn't stored");
    s.execute("style.paragraph.edit", &json!({"name": "Chapter", "rename": "Part"})).unwrap();
    assert_eq!(s.execute("document.epubSplit", &json!({})).unwrap()["split"], json!({"style": "Part"}));
    assert_eq!(files(&mut s, json!({})), 2);
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.execute("document.epubSplit", &json!({})).unwrap()["split"], "off");
}

#[test]
fn object_export_options() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    // A plain rectangle rasterised for EPUB, centred, after a page break.
    let r = s.execute("frame.create", &json!({"rect": [72, 72, 200, 200], "shape": "ellipse"})).unwrap();
    s.execute("object.fill", &json!({"swatch": "[Black]", "ids": [r["id"]]})).unwrap();
    s.execute("object.exportOptions", &json!({"ids": [r["id"]], "rasterize": true, "align": "center", "pageBreakBefore": true, "altText": "A dot"}))
        .unwrap();
    let e = s.execute("file.exportEpub", &json!({})).unwrap();
    let mut z = unzip(e["base64"].as_str().unwrap());
    let x = read(&mut z, "OEBPS/content.xhtml");
    assert!(x.contains("<figure style=\"text-align:center;page-break-before:always;break-before:page;\"><img") && x.contains("alt=\"A dot\""), "{x}");
    // Tagged PDF: an artifact object adds no figure.
    let pdf = |s: &mut Session| {
        String::from_utf8_lossy(&super::super::file::base64_decode(
            s.execute("file.exportPdf", &json!({"tagged": true})).unwrap()["base64"].as_str().unwrap(),
        ))
        .matches("/S/Figure")
        .count()
    };
    let figures = pdf(&mut s);
    s.execute("object.exportOptions", &json!({"ids": [r["id"]], "artifact": true})).unwrap();
    assert!(pdf(&mut s) < figures);
}

#[test]
fn epub_cover_and_fixed_layout() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 2})).unwrap();
    s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "Page one words", "caret": false})).unwrap();
    let r = s.execute("file.exportEpub", &json!({"cover": true})).unwrap();
    assert_eq!(r["warnings"], json!([]), "no images, nothing to report");
    let mut z = unzip(r["base64"].as_str().unwrap());
    let opf = read(&mut z, "OEBPS/content.opf");
    assert!(opf.contains("cover-image") && opf.contains("<itemref idref=\"cover\"/>"));
    assert!(z.by_name("OEBPS/images/cover.png").is_ok());
    let r = s.execute("file.exportFixedEpub", &json!({})).unwrap();
    assert_eq!(r["pages"], 2);
    let mut z = unzip(r["base64"].as_str().unwrap());
    let opf = read(&mut z, "OEBPS/content.opf");
    assert!(opf.contains("rendition:layout\">pre-paginated") && opf.contains("page2.xhtml"));
    let p1 = read(&mut z, "OEBPS/page1.xhtml");
    assert!(p1.contains("width=612, height=792") && p1.contains("Page one words"), "{p1}");
}

/// #775: the fallback follows the font's class, and embedFonts packages the stylesheet's fonts
/// (and reports those it can't).
#[test]
fn epub_font_fallbacks_and_embedding() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 1})).unwrap();
    s.execute("style.paragraph.create", &json!({"name": "Sans", "chars": {"fontFamily": "Source Sans 3", "fontStyle": "Regular"}})).unwrap();
    s.execute("style.paragraph.create", &json!({"name": "Gone", "chars": {"fontFamily": "No Such Font Family", "fontStyle": "Regular"}})).unwrap();
    s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "Words", "caret": false})).unwrap();
    let r = s.execute("file.exportEpub", &json!({})).unwrap();
    let mut z = unzip(r["base64"].as_str().unwrap());
    let css = read(&mut z, "OEBPS/style.css");
    assert!(css.contains("font-family: \"Source Sans 3\", sans-serif;"), "{css}");
    assert!(!css.contains("@font-face") && r["warnings"] == json!([]), "nothing embedded unless asked");
    let r = s.execute("file.exportEpub", &json!({"embedFonts": true})).unwrap();
    let mut z = unzip(r["base64"].as_str().unwrap());
    let css = read(&mut z, "OEBPS/style.css");
    assert!(css.contains("@font-face { font-family: \"Source Sans 3\"; src: url(\"fonts/"), "{css}");
    let opf = read(&mut z, "OEBPS/content.opf");
    let font = opf.split("href=\"fonts/").nth(1).and_then(|r| r.split('"').next()).map(|n| format!("OEBPS/fonts/{n}"));
    assert!(font.as_ref().is_some_and(|f| z.by_name(f).is_ok_and(|e| e.size() > 1000)), "{opf}");
    let warnings = r["warnings"].as_array().unwrap();
    assert!(warnings.iter().any(|w| w.as_str().unwrap().contains("No Such Font Family")), "{warnings:?}");
    let html = s.execute("file.exportHtml", &json!({})).unwrap()["text"].as_str().unwrap().to_string();
    assert!(html.contains("\"Source Sans 3\", sans-serif;"));
}

/// A font whose licence allows embedding bitmaps only (fsType bit 9) is not packaged whole,
/// and a Semibold style asks for the Semibold face's weight, not Bold's.
#[test]
fn epub_embedding_follows_bitmap_only_licences_and_weights() {
    use designcraft_fonts::testing::{font_with, with_fs_type};
    const BITMAP: &str = "DocFont Epub Bitmap Only";
    for fs_type in [0x0200, 0x0208] {
        designcraft_fonts::FontDb::global().add_font(with_fs_type(font_with(&format!("{BITMAP} {fs_type:04x}"), &['W']).unwrap(), fs_type).unwrap());
    }
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 1})).unwrap();
    for fs_type in [0x0200, 0x0208] {
        let family = format!("{BITMAP} {fs_type:04x}");
        s.execute("style.paragraph.create", &json!({"name": family, "chars": {"fontFamily": family, "fontStyle": "Regular"}})).unwrap();
    }
    s.execute("style.paragraph.create", &json!({"name": "Semi", "chars": {"fontFamily": "Source Sans 3", "fontStyle": "Semibold"}})).unwrap();
    s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "Words", "caret": false})).unwrap();
    let r = s.execute("file.exportEpub", &json!({"embedFonts": true})).unwrap();
    let mut z = unzip(r["base64"].as_str().unwrap());
    let css = read(&mut z, "OEBPS/style.css");
    assert!(!css.contains(&format!("@font-face {{ font-family: \"{BITMAP}")), "{css}");
    let warnings = r["warnings"].to_string();
    for fs_type in [0x0200, 0x0208] {
        assert!(warnings.contains(&format!("{BITMAP} {fs_type:04x} Regular`: its licence doesn't allow embedding")), "{warnings}");
    }
    let semi = css.lines().find(|l| l.contains(".semi {")).unwrap_or_default();
    assert!(semi.contains("font-weight: 600;"), "{css}");
}

/// #771, #773: a cover image file and an identifier through the command; the package's
/// modified time is the export's.
#[test]
fn epub_cover_image_file_and_identifier() {
    let dir = std::env::temp_dir().join(format!("dc-epub-cover-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let jpg = dir.join("cover.jpg");
    std::fs::write(&jpg, [0xFF, 0xD8, 0xFF, 0xE0, 0, 16]).unwrap();
    let bad = dir.join("cover.txt");
    std::fs::write(&bad, b"not an image").unwrap();
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 1})).unwrap();
    s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "Words", "caret": false})).unwrap();
    let r = s.execute("file.exportEpub", &json!({"coverImage": jpg.to_str().unwrap(), "identifier": "urn:isbn:9780000000002"})).unwrap();
    let mut z = unzip(r["base64"].as_str().unwrap());
    let opf = read(&mut z, "OEBPS/content.opf");
    assert!(opf.contains("<dc:identifier id=\"bookid\">urn:isbn:9780000000002</dc:identifier>"), "{opf}");
    assert!(opf.contains("href=\"images/cover.jpg\" media-type=\"image/jpeg\""), "{opf}");
    assert!(!opf.contains("2026-01-01T00:00:00Z"), "the export time, not a fixed date: {opf}");
    assert!(z.by_name("OEBPS/images/cover.jpg").is_ok());
    assert!(s.execute("file.exportEpub", &json!({"coverImage": bad.to_str().unwrap()})).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

/// The color spaces of every image XObject in `pdf`, as the families and colorant names.
fn image_spaces(pdf: &[u8]) -> Vec<String> {
    use hayro_syntax::object::{Name, Object};
    let file = hayro_syntax::Pdf::new(pdf.to_vec()).unwrap();
    file.objects()
        .into_iter()
        .filter_map(|o| o.into_stream())
        .filter(|s| s.dict().get::<Name<'_>>(b"Subtype").is_some_and(|n| n.as_ref() == b"Image"))
        .filter_map(|s| match s.dict().get::<Object<'_>>(b"ColorSpace")? {
            Object::Name(n) => Some(n.as_str().to_string()),
            Object::Array(a) => {
                let mut it = a.iter::<Object<'_>>();
                let family = it.next()?.into_name()?.as_str().to_string();
                let names = match it.next() {
                    Some(Object::Array(n)) => n.iter::<Name<'_>>().map(|n| n.as_str().to_string()).collect::<Vec<_>>().join(" "),
                    Some(Object::Name(n)) => n.as_str().to_string(),
                    _ => String::new(),
                };
                Some(format!("{family} {names}"))
            }
            _ => None,
        })
        .collect()
}

/// The (Black, Gold) samples of every flattened DeviceN image that has both colorants.
fn black_and_gold(pdf: &[u8]) -> Vec<(u8, u8)> {
    use hayro_syntax::object::{Array, Name, Object};
    let file = hayro_syntax::Pdf::new(pdf.to_vec()).unwrap();
    let mut out = Vec::new();
    for st in file.objects().into_iter().filter_map(|o| o.into_stream()) {
        let d = st.dict();
        if d.get::<Name<'_>>(b"Subtype").is_none_or(|n| n.as_ref() != b"Image") || d.get::<i32>(b"BitsPerComponent") != Some(8) {
            continue;
        }
        let Some(cs) = d.get::<Array<'_>>(b"ColorSpace") else { continue };
        let mut it = cs.iter::<Object<'_>>();
        if it.next().and_then(|o| o.into_name()).is_none_or(|n| n.as_ref() != b"DeviceN") {
            continue;
        }
        let Some(Object::Array(names)) = it.next() else { continue };
        let names: Vec<String> = names.iter::<Name<'_>>().map(|n| n.as_str().to_string()).collect();
        let (Some(k), Some(g)) = (names.iter().position(|n| n == "Black"), names.iter().position(|n| n == "Gold")) else { continue };
        let data = st.decoded().unwrap();
        out.extend(data.chunks(names.len()).filter(|px| px.len() == names.len()).map(|px| (px[k], px[g])));
    }
    out
}

/// The shadow coverage gives the rendered pixel back when the PDF draws the object over the
/// shadow, also under a partly transparent object (an anti-aliased edge, a soft-alpha image).
#[test]
fn shadow_coverage_composites_back() {
    for a in (0..=255u32).step_by(15) {
        for s in (0..=255u32).step_by(15) {
            let over = |c: u32| a + (c * (255 - a) + 127) / 255;
            let with = over(s) as u8;
            let c = u32::from(super::shadow_coverage(with, a as u8));
            assert!(over(c).abs_diff(u32::from(with)) <= 1, "object {a}, shadow {s}: coverage {c}");
        }
    }
    assert_eq!(super::shadow_coverage(255, 255), 0);
    assert_eq!(super::shadow_coverage(10, 20), 0);
}

/// PDF/X-1a through craft-prepress: a soft drop shadow over a spot, a 50% object and a
/// knockout group leave no transparency and no RGB; the shadow is flattened in the page's
/// inks (Black over the spot), not exported as an RGB image of the object; text stays text and
/// overprint stays `/op`; `flatten` sets the resolution.
#[test]
fn x1a_export_flattens_in_inks() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 1})).unwrap();
    s.execute("swatch.create", &json!({"name": "Gold", "color": {"c": 0, "m": 30, "y": 90, "k": 10}, "spot": true})).unwrap();
    let bg = s.execute("frame.create", &json!({"rect": [36, 36, 576, 756]})).unwrap()["id"].clone();
    s.execute("object.fill", &json!({"ids": [bg], "swatch": "Gold"})).unwrap();
    let shadowed = s.execute("frame.create", &json!({"rect": [72, 72, 200, 200]})).unwrap()["id"].clone();
    s.execute("object.fill", &json!({"ids": [shadowed], "swatch": "C=0 M=100 Y=0 K=0"})).unwrap();
    s.execute("object.dropShadow", &json!({"ids": [shadowed], "on": true, "distance": 8, "size": 6})).unwrap();
    let half = s.execute("frame.create", &json!({"rect": [300, 72, 400, 172]})).unwrap()["id"].clone();
    s.execute("object.fill", &json!({"ids": [half], "swatch": "C=100 M=0 Y=0 K=0"})).unwrap();
    s.execute("object.opacity", &json!({"ids": [half], "opacity": 0.5})).unwrap();
    let a = s.execute("frame.create", &json!({"rect": [72, 300, 172, 400]})).unwrap()["id"].clone();
    let b = s.execute("frame.create", &json!({"rect": [122, 350, 222, 450]})).unwrap()["id"].clone();
    s.execute("object.fill", &json!({"ids": [a, b], "swatch": "C=0 M=0 Y=100 K=0"})).unwrap();
    s.execute("object.opacity", &json!({"ids": [a, b], "opacity": 0.5})).unwrap();
    let g = s.execute("object.group", &json!({"ids": [a, b]})).unwrap()["id"].clone();
    s.execute("object.transparencyGroup", &json!({"ids": [g], "knockout": true})).unwrap();
    let op = s.execute("frame.create", &json!({"rect": [300, 300, 400, 400]})).unwrap()["id"].clone();
    s.execute("object.fill", &json!({"ids": [op], "swatch": "C=0 M=100 Y=0 K=0"})).unwrap();
    s.execute("object.attributes", &json!({"ids": [op], "overprintFill": true})).unwrap();
    s.execute("frame.create", &json!({"rect": [72, 500, 400, 560], "content": "text", "text": "Live text", "caret": false})).unwrap();

    let r = s.execute("file.exportPdf", &json!({"standard": "x1a", "flatten": "low"})).unwrap();
    let pdf = super::super::file::base64_decode(r["base64"].as_str().unwrap());
    let text = String::from_utf8_lossy(&pdf).to_string();
    assert_eq!(designcraft_pdf::check_pdfx1a(&pdf), Vec::<String>::new());
    assert!(pdf.starts_with(b"%PDF-1.4") && text.contains("/GTS_PDFXVersion(PDF/X-1:2003)") && text.contains("/S/GTS_PDFX"));
    assert!(text.contains("/GTS_PDFXConformance(PDF/X-1a:2003)"), "the conformance level");
    let warnings = r["warnings"].to_string();
    assert!(!warnings.contains("soft effects"), "the shadow isn't an RGB image of the object: {warnings}");
    assert!(warnings.contains("at 72 ppi"), "{warnings}");
    let spaces = image_spaces(&pdf);
    assert!(spaces.iter().all(|cs| !cs.contains("RGB") && !cs.contains("ICC")), "{spaces:?}");
    assert!(spaces.iter().any(|cs| cs.starts_with("DeviceN") && cs.contains("Black") && cs.contains("Gold")), "the shadow over the spot: {spaces:?}");
    // The shadow multiplies: the spot stays at 100% where the shadow's black is.
    let shadowed: Vec<(u8, u8)> = black_and_gold(&pdf).into_iter().filter(|(k, _)| *k > 100).collect();
    assert!(!shadowed.is_empty(), "no shadow pixels");
    assert!(shadowed.iter().all(|(_, gold)| *gold >= 250), "the spot under the shadow: {:?}", &shadowed[..shadowed.len().min(8)]);
    assert!(text.contains("/op true") && text.contains("/OPM 1"), "overprint");
    assert!(text.contains("/FontFile"), "the text is still text");
    // The default resolution.
    let r = s.execute("file.exportPdf", &json!({"standard": "x1a"})).unwrap();
    assert!(r["warnings"].to_string().contains("at 300 ppi"), "{}", r["warnings"]);
}

#[test]
fn flattener_rasterises_spreads_with_transparency() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 3})).unwrap();
    let id = s.execute("frame.create", &json!({"rect": [100, 100, 300, 300]})).unwrap()["id"].clone();
    s.execute("object.fill", &json!({"swatch": "[Black]", "ids": [id]})).unwrap();
    // Opaque: nothing to flatten.
    let r = s.execute("file.exportPdf", &json!({"flatten": "low"})).unwrap();
    assert!(r["warnings"].as_array().unwrap().iter().all(|w| !w.as_str().unwrap().contains("flattened")));
    s.execute("object.opacity", &json!({"ids": [id], "opacity": 0.5})).unwrap();
    // Text away from the transparency stays live text.
    s.execute("frame.create", &json!({"rect": [72, 400, 400, 450], "content": "text", "text": "Still vector", "caret": false})).unwrap();
    let r = s.execute("file.exportPdf", &json!({"flatten": "low"})).unwrap();
    let pdf = super::super::file::base64_decode(r["base64"].as_str().unwrap());
    assert!(String::from_utf8_lossy(&pdf).contains("/FontFile"), "the text is still text");
    assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("flattened: 1 ")), "{r}");
    assert_eq!(r["pages"], 3);
    let bytes = super::super::file::base64_decode(r["base64"].as_str().unwrap());
    assert_eq!(designcraft_render::pdf_page_count(&bytes), Some(3));
    // The document itself is untouched.
    assert!(s.doc().unwrap().doc.item(designcraft_doc::ItemId(id.as_u64().unwrap())).is_some());
    assert!(s.execute("file.exportPdf", &json!({"flatten": 5})).is_err());
    // Interactive view options land in the catalog and on the pages.
    let r = s.execute("file.exportPdf", &json!({"fullScreen": true, "pageLayout": "single", "view": "fitPage", "advanceSeconds": 5})).unwrap();
    let t = String::from_utf8_lossy(&super::super::file::base64_decode(r["base64"].as_str().unwrap())).to_string();
    assert!(
        t.contains("/PageMode/FullScreen") && t.contains("/PageLayout/SinglePage") && t.contains("/OpenAction[") && t.contains("/Dur 5.00"),
        "view options"
    );
    assert!(s.execute("file.exportPdf", &json!({"pageLayout": "sideways"})).is_err());
    // Unflattened, the page blends in the document's blend space (CMYK for print).
    let r = s.execute("file.exportPdf", &json!({})).unwrap();
    let text = String::from_utf8_lossy(&super::super::file::base64_decode(r["base64"].as_str().unwrap())).to_string();
    assert_eq!(text.matches("/S/Transparency/CS/DeviceCMYK").count(), 1, "only the page with transparency");
}

#[test]
fn objects_with_soft_effects_keep_their_text_wrap_in_pdf() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let text = "Words wrap around the objects on this page. ".repeat(60);
    let t = s.execute("frame.create", &json!({"rect": [36, 36, 576, 756], "content": "text", "text": text, "caret": false})).unwrap()["id"]
        .as_u64()
        .unwrap();
    // A feathered frame with a contour wrap of the frame itself.
    let a = s.execute("frame.create", &json!({"rect": [100, 100, 200, 200]})).unwrap()["id"].clone();
    s.execute("object.fill", &json!({"swatch": "[Black]", "ids": [a]})).unwrap();
    s.execute("object.textWrap", &json!({"ids": [a], "mode": "contour", "contour": "sameAsClipping", "offset": 6})).unwrap();
    // A feathered frame whose placed graphic has a bounding-box wrap.
    let png = designcraft_render::Rendered { width: 40, height: 40, pixels: vec![90; 40 * 40 * 4] }.to_png();
    let b = s.execute("frame.create", &json!({"rect": [300, 400, 400, 500]})).unwrap()["id"].as_u64().unwrap();
    s.execute("file.place", &json!({"base64": super::super::file::base64_encode(&png), "name": "a.png", "frame": b})).unwrap();
    s.execute("object.textWrap", &json!({"ids": [b], "content": true, "mode": "boundingBox", "offset": 6})).unwrap();
    s.execute("object.feather", &json!({"ids": [a, b], "width": 14})).unwrap();

    let d = &s.doc().unwrap().doc;
    let sid = d.item(designcraft_doc::ItemId(t)).and_then(|it| it.text_frame()).unwrap().story;
    let before = designcraft_compose::frame_specs(d, sid)[0].exclusions.clone();
    assert_eq!(before.len(), 2);
    let (fx, n) = super::rasterize_effects(d, &s.cache, &Default::default());
    assert_eq!(n, 2);
    let fx = fx.unwrap();
    assert_eq!(designcraft_compose::frame_specs(&fx, sid)[0].exclusions, before);
    // The objects that keep the wrap don't print; their images do.
    let sp = &fx.spreads[0];
    assert_eq!(sp.items.iter().filter(|it| it.nonprinting).count(), 2);
    assert_eq!(sp.items.iter().filter(|it| !it.nonprinting && it.graphic().is_some()).count(), 2);
}

#[test]
fn export_story_as_text_and_rtf() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "One\nTwo"})).unwrap();
    assert!(s.execute("file.exportText", &json!({"frame": r["id"]})).unwrap()["text"] == "One\r\nTwo");
    let html = s.execute("file.exportHtml", &json!({})).unwrap();
    assert!(html["text"].as_str().unwrap().contains("One</p>"));
    let tagged = s.execute("file.exportText", &json!({"frame": r["id"], "format": "tagged"})).unwrap();
    assert!(tagged["text"].as_str().unwrap().contains("<ParaStyle:"));
    let rtf = s.execute("file.exportText", &json!({"frame": r["id"], "format": "rtf"})).unwrap();
    assert!(rtf["text"].as_str().unwrap().starts_with("{\\rtf1"));
    let dir = std::env::temp_dir().join(format!("dc-export-text-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("story.rtf");
    s.execute("file.exportText", &json!({"frame": r["id"], "path": path.to_string_lossy()})).unwrap();
    assert!(std::fs::read_to_string(&path).unwrap().starts_with("{\\rtf1"));
    std::fs::remove_dir_all(&dir).ok();
}
