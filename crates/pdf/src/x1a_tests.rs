//! PDF/X-1a export, read back from the exported PDF: no transparency left, only process and spot
//! color, overprint and live text kept, the identification; and the checks refusing what X-1a
//! doesn't allow.

use std::collections::HashMap;
use std::sync::Arc;

use designcraft_color::{Color, ColorType, Swatch, SwatchValue};
use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Content, Document, Fill, Item, ItemId, ParaFormat, SpreadRef};
use designcraft_geom::Rect;
use hayro_syntax::Pdf;
use hayro_syntax::object::{Dict, Name, Object};

use crate::tests::extract_text;
use crate::{PdfOptions, ShadowMask, Standard, check_pdfx1a, export_pdf_with_report};

fn add(d: &mut Document, r: Rect, swatch: &str, edit: impl FnOnce(&mut Item)) -> ItemId {
    let lid = d.default_layer();
    let id = ItemId(d.alloc());
    let mut b = Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(r));
    b.fill = Fill::swatch(swatch);
    edit(&mut b);
    d.insert_item(SpreadRef::Doc(0), b, None).unwrap();
    id
}

/// A spot Gold background with, over it: a box with a soft drop shadow, a 50% cyan box, a
/// knockout group of two 50% boxes, a magenta box that overprints, and a text frame.
fn transparent_doc() -> (Document, PdfOptions) {
    let mut d = Document::new(&NewDocument::default());
    let gold = Color::cmyk(0.0, 0.3, 0.9, 0.1);
    d.swatches.push(Swatch { value: SwatchValue::Color { color: gold, color_type: ColorType::Spot }, ..Swatch::color("Gold", gold) });
    add(&mut d, Rect::new(0.0, 0.0, 612.0, 792.0), "Gold", |_| {});
    let shadowed = add(&mut d, Rect::new(72.0, 72.0, 172.0, 172.0), "C=0 M=100 Y=0 K=0", |b| {
        b.effects.drop_shadow.on = true;
        b.effects.drop_shadow.opacity = 1.0;
    });
    add(&mut d, Rect::new(300.0, 72.0, 400.0, 172.0), "C=100 M=0 Y=0 K=0", |b| b.opacity = 0.5);
    let lid = d.default_layer();
    let child = |d: &mut Document, r: Rect, swatch: &str| {
        let mut b = Item::new(ItemId(d.alloc()), lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(r));
        b.fill = Fill::swatch(swatch);
        b.opacity = 0.5;
        Arc::new(b)
    };
    let items = vec![
        child(&mut d, Rect::new(72.0, 300.0, 172.0, 400.0), "C=100 M=0 Y=0 K=0"),
        child(&mut d, Rect::new(122.0, 350.0, 222.0, 450.0), "C=0 M=0 Y=100 K=0"),
    ];
    let mut g =
        Item::new(ItemId(d.alloc()), lid, designcraft_doc::Shape::Group, designcraft_geom::shapes::rectangle(Rect::new(72.0, 300.0, 222.0, 450.0)));
    g.content = Content::Group { items };
    g.knockout = true;
    d.insert_item(SpreadRef::Doc(0), g, None).unwrap();
    add(&mut d, Rect::new(300.0, 300.0, 400.0, 400.0), "C=0 M=100 Y=0 K=0", |b| b.fill.overprint = true);
    d.add_text_frame(SpreadRef::Doc(0), Rect::new(72.0, 500.0, 400.0, 600.0), lid, "Live text", ParaFormat::default()).unwrap();
    // The shadow's coverage: a ramp from 0 to full strength, offset from the box.
    let (w, h) = (64u32, 64u32);
    let coverage: Vec<u8> = (0..h).flat_map(|_| (0..w).map(move |x| (x * 255 / (w - 1)) as u8)).collect();
    let mask = ShadowMask { rect: Rect::new(77.0, 77.0, 182.0, 182.0), width: w, height: h, coverage: Arc::new(coverage) };
    let opts =
        PdfOptions { standard: Standard::PdfX1a, flatten_dpi: 72.0, shadows: Arc::new(HashMap::from([(shadowed, mask)])), ..PdfOptions::default() };
    (d, opts)
}

/// Every color space family named in the file's resources, images and shadings.
fn families(bytes: &[u8]) -> Vec<String> {
    let pdf = Pdf::new(bytes.to_vec()).unwrap();
    let mut out = Vec::new();
    let mut add = |o: Option<Object<'_>>| {
        let name = match o {
            Some(Object::Name(n)) => Some(n.as_str().to_string()),
            Some(Object::Array(a)) => a.iter::<Name<'_>>().next().map(|n| n.as_str().to_string()),
            _ => None,
        };
        if let Some(n) = name
            && !out.contains(&n)
        {
            out.push(n);
        }
    };
    for o in pdf.objects() {
        let dict: Option<Dict<'_>> = match o {
            Object::Dict(d) => Some(d),
            Object::Stream(s) => Some(s.dict().clone()),
            _ => None,
        };
        let Some(dict) = dict else { continue };
        add(dict.get::<Object<'_>>(b"ColorSpace").filter(|o| !matches!(o, Object::Dict(_))));
        if let Some(spaces) = dict.get::<Dict<'_>>(b"ColorSpace") {
            for key in spaces.keys() {
                add(spaces.get::<Object<'_>>(key.as_ref()));
            }
        }
    }
    out
}

/// A drop shadow over a spot, a 50% object and a knockout group come out with no transparency,
/// only process and spot color, the output intent and the PDF/X-1a identification; overprint
/// and live text survive.
#[test]
fn x1a_export_flattens_transparency_into_inks() {
    let (d, opts) = transparent_doc();
    let r = export_pdf_with_report(&d, &Cache::new(), &opts).unwrap();
    let bytes = &r.bytes;
    let text = String::from_utf8_lossy(bytes);
    assert!(bytes.starts_with(b"%PDF-1.4"), "{:?}", text.get(..8));
    assert_eq!(check_pdfx1a(bytes), Vec::<String>::new());
    assert!(text.contains("/GTS_PDFXVersion(PDF/X-1:2003)") && text.contains("/GTS_PDFXConformance(PDF/X-1a:2003)"), "the identification");
    assert!(text.contains("<pdfx:GTS_PDFXConformance>PDF/X-1a:2003</pdfx:GTS_PDFXConformance>"), "the identification in XMP");
    // The default Coated FOGRA39 is a registered condition: no (ICC v4) profile in the file.
    let pdf = Pdf::new(bytes.to_vec()).unwrap();
    let oi = pdfx_intent(&pdf);
    assert_eq!(oi.get::<hayro_syntax::object::String<'_>>(b"OutputConditionIdentifier").unwrap().as_bytes(), b"FOGRA39");
    assert_eq!(oi.get::<hayro_syntax::object::String<'_>>(b"RegistryName").unwrap().as_bytes(), b"http://www.color.org");
    assert!(!oi.contains_key(b"DestOutputProfile"), "no destination profile");
    let profiles = pdf.objects().into_iter().filter(|o| matches!(o, Object::Stream(s) if s.dict().get::<i32>(b"N") == Some(4))).count();
    assert_eq!(profiles, 0, "the profile is left out of the file");
    assert!(!text.contains("/NChannel"), "no PDF 1.6 NChannel attributes at PDF 1.4");
    assert!(text.contains("/Trapped/False"), "Trapped");
    for f in families(bytes) {
        assert!(["DeviceCMYK", "DeviceGray", "Separation", "DeviceN", "Indexed", "Pattern"].contains(&f.as_str()), "color space {f}");
    }
    assert!(text.contains("/Gold"), "the spot ink stays a spot");
    for gone in ["/SMask<<", "/SMask <<", "/S/Transparency", "/ca 0.5", "/CA 0.5", "/BM/Multiply"] {
        assert!(!text.contains(gone), "{gone}");
    }
    let states: Vec<(bool, bool, i32)> = pdf
        .objects()
        .into_iter()
        .filter_map(|o| match o {
            Object::Dict(d) => Some((d.get::<bool>(b"op")?, d.get::<bool>(b"OP")?, d.get::<i32>(b"OPM")?)),
            _ => None,
        })
        .collect();
    assert!(states.iter().any(|(op, _, opm)| *op && *opm == 1), "overprint survives: {states:?}");
    assert!(extract_text(bytes).concat().contains("Live text"), "{:?}", extract_text(bytes));
}

/// The catalog's `GTS_PDFX` output intent.
fn pdfx_intent<'a>(pdf: &'a Pdf) -> Dict<'a> {
    let xref = pdf.xref();
    let catalog = xref.get::<Dict<'_>>(xref.root_id()).unwrap();
    let intents = catalog.get::<hayro_syntax::object::Array<'_>>(b"OutputIntents").unwrap();
    intents.iter::<Dict<'_>>().find(|oi| oi.get::<Name<'_>>(b"S").is_some_and(|s| s.as_ref() == b"GTS_PDFX")).unwrap()
}

/// An ICC profile header of major version `major`, all a check reads of it.
fn icc_header(major: u8) -> Vec<u8> {
    [&[0u8; 8][..], &[major, 0x20, 0, 0], b"prtrCMYK"].concat()
}

/// A minimal PDF 1.4 with a CMYK output intent (an unregistered condition with an ICC v2
/// profile), the X-1a identification and `content` on its page; `entries` go into the page
/// dictionary.
fn tiny_x1a(entries: &str, content: &str) -> Vec<u8> {
    tiny_x1a_with(entries, content, "/OutputConditionIdentifier(Custom)/DestOutputProfile 4 0 R", &icc_header(2))
}

/// [`tiny_x1a`] whose output intent (object 7) has `intent` and whose object 4 holds `profile`.
fn tiny_x1a_with(entries: &str, content: &str, intent: &str, profile: &[u8]) -> Vec<u8> {
    assemble(&tiny_objects(entries, content, intent, profile))
}

/// [`tiny_x1a_with`]'s objects, from 1.
fn tiny_objects(entries: &str, content: &str, intent: &str, profile: &[u8]) -> Vec<Vec<u8>> {
    vec![
        b"<</Type/Catalog/Pages 2 0 R/OutputIntents[7 0 R]>>".to_vec(),
        b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
        format!("<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/TrimBox[0 0 100 100]{entries}/Contents 5 0 R>>").into_bytes(),
        [format!("<</N 4/Length {}>>\nstream\n", profile.len()).as_bytes(), profile, b"\nendstream"].concat(),
        format!("<</Length {}>>\nstream\n{content}\nendstream", content.len()).into_bytes(),
        b"<</GTS_PDFXVersion(PDF/X-1:2003)/GTS_PDFXConformance(PDF/X-1a:2003)/Trapped/False>>".to_vec(),
        format!("<</Type/OutputIntent/S/GTS_PDFX{intent}>>").into_bytes(),
    ]
}

/// A PDF 1.4 file of `objects` (from 1), object 1 the catalog and 6 the information dictionary.
fn assemble(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f\r\n", objects.len() + 1).as_bytes());
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<</Size {}/Root 1 0 R/Info 6 0 R>>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
    out
}

/// The X-1a checks refuse soft masks, constant opacity, blend modes, transparency groups, RGB
/// and ICC-based color; process and spot color pass.
#[test]
fn x1a_checks_refuse_transparency_and_rgb() {
    let clean = check_pdfx1a(&tiny_x1a(
        "/Resources<</ColorSpace<</Gold[/Separation/Gold/DeviceCMYK 9 0 R]>>>>",
        "0 0 0 1 k 0 0 10 10 re f 0.5 G /Gold cs 1 scn 0 0 5 5 re f",
    ));
    assert_eq!(clean, Vec::<String>::new());
    let has = |entries: &str, content: &str, what: &str| {
        let issues = check_pdfx1a(&tiny_x1a(entries, content));
        assert!(issues.iter().any(|i| i.contains(what)), "{what}: {issues:?}");
    };
    has("/Resources<<>>", "1 0 0 rg 0 0 10 10 re f", "DeviceRGB");
    has("/Resources<</ColorSpace<</CS0[/ICCBased 4 0 R]>>>>", "/CS0 cs 0 0 0 1 scn 0 0 10 10 re f", "ICC-based");
    has("/Resources<</ExtGState<</G0<</SMask<</S/Luminosity/G 5 0 R>>>>>>>>", "/G0 gs 0 0 0 1 k 0 0 10 10 re f", "soft mask");
    has("/Resources<</ExtGState<</G0<</ca 0.5>>>>>>", "/G0 gs 0 0 0 1 k 0 0 10 10 re f", "opacity");
    has("/Resources<</ExtGState<</G0<</BM/Multiply>>>>>>", "/G0 gs 0 0 0 1 k 0 0 10 10 re f", "/Multiply");
    has("/Resources<<>>/Group<</S/Transparency/CS/DeviceCMYK>>", "0 0 0 1 k", "transparency group");
    has("/Resources<</ColorSpace<</S[/Separation/Gold/DeviceRGB 9 0 R]>>>>", "/S cs 1 scn 0 0 10 10 re f", "alternate space");
    has("/Resources<<>>", "q BI /W 1 /H 1 /CS /RGB /BPC 8 /F /AHx ID ff0000> EI Q", "DeviceRGB");
    has(
        "/Resources<</ColorSpace<</N[/DeviceN[/Cyan/Gold]/DeviceCMYK 9 0 R<</Subtype/NChannel/Colorants<<>>>>]>>>>",
        "/N cs 1 1 scn 0 0 10 10 re f",
        "NChannel",
    );
    // The output intent: an ICC v2 profile, or none for a registered condition.
    let intent = |intent: &str, profile: &[u8]| check_pdfx1a(&tiny_x1a_with("/Resources<<>>", "0 0 0 1 k", intent, profile));
    let v4 = intent("/OutputConditionIdentifier(Custom)/DestOutputProfile 4 0 R", &icc_header(4));
    assert!(v4.iter().any(|i| i.contains("ICC v4")), "{v4:?}");
    let registered = "/OutputConditionIdentifier(FOGRA39)/RegistryName(http://www.color.org)";
    assert!(!intent(&format!("{registered}/DestOutputProfile 4 0 R"), &icc_header(4)).is_empty(), "v4 even when registered");
    assert_eq!(intent(registered, b""), Vec::<String>::new());
    let unnamed = intent("/OutputConditionIdentifier(Custom)", b"");
    assert!(unnamed.iter().any(|i| i.contains("no destination profile")), "{unnamed:?}");
    // The identification names the conformance level.
    let base_only = String::from_utf8_lossy(&tiny_x1a("/Resources<<>>", "0 0 0 1 k")).replace("/GTS_PDFXConformance(PDF/X-1a:2003)", "");
    let issues = check_pdfx1a(base_only.as_bytes());
    assert!(issues.iter().any(|i| i.contains("PDF/X-1a:2003")), "{issues:?}");
    // A Type 3 font is embedded by nature; its glyphs are checked like the page.
    let type3 = |glyph: &str| {
        let font = "/Resources<</Font<</F0<</Type/Font/Subtype/Type3/FontBBox[0 0 1 1]/FontMatrix[1 0 0 1 0 0]/CharProcs<</a 8 0 R>>\
                    /Encoding<</Type/Encoding/Differences[97/a]>>/FirstChar 97/LastChar 97/Widths[1]>>>>>>";
        let mut objects = tiny_objects(font, "BT /F0 10 Tf (a) Tj ET", "/OutputConditionIdentifier(Custom)/DestOutputProfile 4 0 R", &icc_header(2));
        objects.push(format!("<</Length {}>>\nstream\n{glyph}\nendstream", glyph.len()).into_bytes());
        check_pdfx1a(&assemble(&objects))
    };
    assert_eq!(type3("1 0 d0 0 0 0 1 k 0 0 1 1 re f"), Vec::<String>::new());
    let rgb = type3("1 0 d0 1 0 0 rg 0 0 1 1 re f");
    assert!(rgb.iter().any(|i| i.contains("DeviceRGB")), "{rgb:?}");
    // A PDF/X-4 file isn't X-1a.
    let x4 = export_pdf_with_report(
        &Document::new(&NewDocument::default()),
        &Cache::new(),
        &PdfOptions { standard: Standard::PdfX4, ..Default::default() },
    )
    .unwrap();
    let issues = check_pdfx1a(&x4.bytes);
    assert!(issues.iter().any(|i| i.contains("1.4")) && issues.iter().any(|i| i.contains("PDF/X-1:2003")), "{issues:?}");
}

#[test]
fn x1a_parses() {
    for s in ["x1a", "PDF/X-1a", "PDF/X-1a:2003", "pdfx1a"] {
        assert_eq!(Standard::parse(s), Some(Standard::PdfX1a), "{s}");
    }
}

/// The identification and output intent krilla doesn't write: the conformance level goes into
/// the document information; a registered condition's intent loses its profile, which leaves the
/// file; an unregistered condition keeps an ICC v2 profile and refuses a v4 one.
#[test]
fn x1a_finish_names_the_conformance_and_fixes_the_intent() {
    let base = |intent: &str, major: u8| {
        let mut objects = tiny_objects("/Resources<<>>", "0 0 0 1 k", intent, &icc_header(major));
        objects[5] = b"<</GTS_PDFXVersion(PDF/X-1:2003)/Trapped/False>>".to_vec();
        assemble(&objects)
    };
    let registered = "/OutputConditionIdentifier(FOGRA39)/RegistryName(http://www.color.org)/DestOutputProfile 4 0 R";
    let before = base(registered, 4);
    assert!(!check_pdfx1a(&before).is_empty());
    let out = crate::pdfx1a::finish(&before).unwrap();
    assert_eq!(check_pdfx1a(&out), Vec::<String>::new());
    let pdf = Pdf::new(out.clone()).unwrap();
    assert!(!pdfx_intent(&pdf).contains_key(b"DestOutputProfile"));
    assert!(!String::from_utf8_lossy(&out).contains("/N 4"), "the profile is left out");
    assert_eq!(pdf.pages().len(), 1);

    let custom = "/OutputConditionIdentifier(Custom)/OutputCondition(Press)/DestOutputProfile 4 0 R";
    let kept = crate::pdfx1a::finish(&base(custom, 2)).unwrap();
    assert_eq!(check_pdfx1a(&kept), Vec::<String>::new());
    assert!(pdfx_intent(&Pdf::new(kept).unwrap()).contains_key(b"DestOutputProfile"));
    let refused = crate::pdfx1a::finish(&base(custom, 4)).unwrap_err();
    assert!(refused.contains("`Press`") && refused.contains("ICC v4"), "{refused}");
}

/// An image the flattener can't read fails the export, naming the page and its graphics.
#[test]
fn x1a_refuses_to_leave_images_out() {
    let objects = vec![
        b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
        b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
        b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/TrimBox[0 0 100 100]/Resources<</XObject<</Im0 4 0 R>>>>/Contents 5 0 R>>".to_vec(),
        b"<</Type/XObject/Subtype/Image/Width 2/Height 2/ColorSpace/DeviceCMYK/BitsPerComponent 8/Filter/DCTDecode/Length 8>>\nstream\nnot jpeg\nendstream".to_vec(),
        b"<</Length 30>>\nstream\nq 50 0 0 50 10 10 cm /Im0 Do Q\nendstream".to_vec(),
        b"<</Producer(test)>>".to_vec(),
    ];
    let pages = [crate::pdfx1a::PageGraphics { label: "iv".into(), graphics: vec!["logo.pdf".into()] }];
    let err = crate::pdfx1a::flatten_x1a(&assemble(&objects), 72.0, &pages, &mut Vec::new()).unwrap_err().to_string();
    assert!(err.contains("1 image on page iv") && err.contains("`logo.pdf`"), "{err}");
}

/// Spot inks past the flattener's ink set lose their tints: the warning says they won't print.
#[test]
fn x1a_dropped_inks_warning_says_they_wont_print() {
    let mut totals = craft_prepress::flatten::FlattenTotals::default();
    totals.dropped_inks = 3;
    let warnings = crate::pdfx1a::flatten_warnings(&totals, 300.0);
    assert!(warnings.iter().any(|w| w.contains("3 tint(s)") && w.contains("dropped and won't print")), "{warnings:?}");
    assert!(warnings.iter().all(|w| !w.contains("converted to process")), "{warnings:?}");
}
