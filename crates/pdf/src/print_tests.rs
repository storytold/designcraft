//! CMYK print output, read back from the exported PDF: blending spaces, the output intent,
//! overprint and knockout groups.

use designcraft_color::cms::{self, Cms, ColorSettings};
use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{BlendSpace, Document, Fill, Item, ItemId, SpreadRef};
use designcraft_geom::Rect;
use hayro_syntax::Pdf;
use hayro_syntax::object::{Array, Dict, Name, Object, Stream};

use crate::{PdfOptions, export_pdf};

/// A box filled with `swatch`; `edit` sets its other attributes.
fn add_box(d: &mut Document, r: Rect, swatch: &str, edit: impl FnOnce(&mut Item)) -> ItemId {
    let lid = d.default_layer();
    let id = ItemId(d.alloc());
    let mut b = Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(r));
    b.fill = Fill::swatch(swatch);
    edit(&mut b);
    d.insert_item(SpreadRef::Doc(0), b, None).unwrap();
    id
}

/// One page: a cyan box with a 50% black box over it at 50% opacity.
fn transparent_doc() -> Document {
    let mut d = Document::new(&NewDocument::default());
    add_box(&mut d, Rect::new(36.0, 36.0, 236.0, 236.0), "C=100 M=0 Y=0 K=0", |_| {});
    add_box(&mut d, Rect::new(136.0, 136.0, 336.0, 336.0), "Black", |b| b.opacity = 0.5);
    d
}

pub(crate) struct Intent {
    pub identifier: String,
    pub condition: String,
    pub registry: Option<String>,
    pub profile: Vec<u8>,
}

fn text(d: &Dict<'_>, key: &[u8]) -> Option<String> {
    d.get::<hayro_syntax::object::String>(key).map(|s| String::from_utf8_lossy(s.as_bytes()).into_owned())
}

/// The catalog's `GTS_PDFX` output intent.
pub(crate) fn output_intent(bytes: &[u8]) -> Option<Intent> {
    let pdf = Pdf::new(bytes.to_vec()).ok()?;
    let xref = pdf.xref();
    let catalog = xref.get::<Dict<'_>>(xref.root_id())?;
    let intents = catalog.get::<Array<'_>>(b"OutputIntents")?;
    let oi = intents.iter::<Dict<'_>>().find(|oi| oi.get::<Name<'_>>(b"S").is_some_and(|s| s.as_ref() == b"GTS_PDFX"))?;
    Some(Intent {
        identifier: text(&oi, b"OutputConditionIdentifier")?,
        condition: text(&oi, b"OutputCondition").unwrap_or_default(),
        registry: text(&oi, b"RegistryName"),
        profile: oi.get::<Stream<'_>>(b"DestOutputProfile")?.decoded().ok()?.into_owned(),
    })
}

/// The `/CS` of each page's transparency group (`None` for a page without one).
fn page_group_spaces(bytes: &[u8]) -> Vec<Option<String>> {
    let pdf = Pdf::new(bytes.to_vec()).unwrap();
    pdf.pages()
        .iter()
        .map(|p| {
            let g = p.raw().get::<Dict<'_>>(b"Group")?;
            match g.get::<Object<'_>>(b"CS")? {
                Object::Name(n) => Some(n.as_str().to_string()),
                Object::Array(a) => a.iter::<Name<'_>>().next().map(|n| n.as_str().to_string()),
                _ => None,
            }
        })
        .collect()
}

/// Every form XObject's transparency group dictionary.
fn form_groups(bytes: &[u8]) -> Vec<(Option<String>, bool, bool)> {
    let pdf = Pdf::new(bytes.to_vec()).unwrap();
    pdf.objects()
        .into_iter()
        .filter_map(|o| o.into_stream())
        .filter(|s| s.dict().get::<Name<'_>>(b"Subtype").is_some_and(|n| n.as_ref() == b"Form"))
        .filter_map(|s| {
            let g = s.dict().get::<Dict<'_>>(b"Group")?;
            let cs = g.get::<Name<'_>>(b"CS").map(|n| n.as_str().to_string());
            Some((cs, g.get::<bool>(b"I").unwrap_or(false), g.get::<bool>(b"K").unwrap_or(false)))
        })
        .collect()
}

/// Print output (a CMYK blend space) blends in DeviceCMYK. A plain export carries no output
/// intent and no `/Trapped`, as in InDesign; PDF/X-4 carries the CMYK working space as its output
/// intent with `/Trapped /False`. A screen document (RGB blend space) is written as before.
#[test]
fn print_output_blends_in_cmyk_with_an_output_intent_only_in_pdfx() {
    let mut d = transparent_doc();
    let bytes = export_pdf(&d, &Cache::new(), &PdfOptions::default()).unwrap();
    assert_eq!(page_group_spaces(&bytes), [Some("DeviceCMYK".to_string())]);
    assert!(!form_groups(&bytes).is_empty());
    assert!(form_groups(&bytes).iter().all(|(cs, ..)| cs.is_none()), "content groups inherit the page group: {:?}", form_groups(&bytes));
    assert!(!crate::has_rgb_groups(&bytes));
    assert!(output_intent(&bytes).is_none(), "no output intent outside PDF/X");
    assert!(!String::from_utf8_lossy(&bytes).contains("/OutputIntents"));
    assert!(!String::from_utf8_lossy(&bytes).contains("/Trapped"));

    let x4 = export_pdf(&d, &Cache::new(), &PdfOptions { standard: crate::Standard::PdfX4, ..PdfOptions::default() }).unwrap();
    assert_eq!(page_group_spaces(&x4), [Some("DeviceCMYK".to_string())]);
    let intent = output_intent(&x4).expect("output intent");
    assert_eq!(intent.identifier, "FOGRA39");
    assert_eq!(intent.profile, craft_cms::builtin::COATED_FOGRA39_ICC);
    assert!(String::from_utf8_lossy(&x4).contains("/Trapped/False"));

    // A booklet of a print document blends in CMYK too, without an output intent.
    let booklet = crate::export_booklet(&d, &Cache::new(), &crate::BookletOptions::default()).unwrap();
    assert!(page_group_spaces(&booklet.bytes).contains(&Some("DeviceCMYK".to_string())));
    assert!(output_intent(&booklet.bytes).is_none());

    d.settings.blend_space = BlendSpace::Rgb;
    let screen = export_pdf(&d, &Cache::new(), &PdfOptions::default()).unwrap();
    assert!(output_intent(&screen).is_none());
    assert_eq!(page_group_spaces(&screen), [None]);
    assert!(!String::from_utf8_lossy(&screen).contains("/Trapped"));
}

/// The output intent is the CMYK working space's profile and condition, not FOGRA39.
#[test]
fn output_intent_is_the_cmyk_working_space() {
    use craft_pdf::write::backend as krilla;
    let generic = Cms::new(&ColorSettings { cmyk: cms::GENERIC_CMYK.into(), ..ColorSettings::default() }).unwrap();
    let (plain, warning) = crate::pdfx::print_settings_with(&generic, false).unwrap();
    assert_eq!(warning, None);
    assert!(plain.output_intent.is_none() && plain.trapped.is_none() && plain.cmyk_profile.is_some());
    let (settings, warning) = crate::pdfx::print_settings_with(&generic, true).unwrap();
    assert_eq!(warning, None);
    let mut pdf = krilla::Document::new_with(settings);
    let mut page = pdf.start_page_with(krilla::page::PageSettings::new(krilla::geom::Size::from_wh(100.0, 100.0).unwrap()));
    page.surface().finish();
    page.finish();
    let bytes = pdf.finish().unwrap();
    let intent = output_intent(&bytes).expect("output intent");
    let want = generic.output_intent().unwrap();
    assert_eq!((intent.identifier.as_str(), intent.condition.as_str()), ("Custom", cms::GENERIC_CMYK));
    assert_eq!(intent.registry, None, "an unregistered condition names no registry");
    assert_eq!(intent.profile, &*want.icc);
    assert_ne!(intent.profile, craft_cms::builtin::COATED_FOGRA39_ICC);
}

/// The overprint entries of every ExtGState that has them: (`/op`, `/OP`, `/OPM`).
fn overprint_states(bytes: &[u8]) -> Vec<(bool, bool, i32)> {
    let pdf = Pdf::new(bytes.to_vec()).unwrap();
    pdf.objects()
        .into_iter()
        .filter_map(|o| match o {
            Object::Dict(d) => Some(d),
            _ => None,
        })
        .filter_map(|d| Some((d.get::<bool>(b"op")?, d.get::<bool>(b"OP")?, d.get::<i32>(b"OPM")?)))
        .collect()
}

/// A document whose [Black] doesn't overprint by itself, with one box `edit` sets up.
fn one_box(swatch: &str, edit: impl FnOnce(&mut Item)) -> Document {
    let mut d = Document::new(&NewDocument::default());
    d.settings.overprint_black = false;
    add_box(&mut d, Rect::new(36.0, 36.0, 236.0, 236.0), swatch, edit);
    d
}

fn exported(d: &Document) -> Vec<u8> {
    export_pdf(d, &Cache::new(), &PdfOptions::default()).unwrap()
}

/// Overprint Fill, Overprint Stroke and Overprint Gap are written as `/op` and `/OP` with
/// `/OPM 1`; without them nothing overprints.
#[test]
fn overprint_attributes_are_written() {
    const CYAN: &str = "C=100 M=0 Y=0 K=0";
    let stroked = |b: &mut Item| {
        b.stroke.swatch = "C=0 M=100 Y=0 K=0".into();
        b.stroke.weight = 4.0;
    };
    assert_eq!(overprint_states(&exported(&one_box(CYAN, stroked))), []);
    let fill = exported(&one_box(CYAN, |b| b.fill.overprint = true));
    assert_eq!(overprint_states(&fill), [(true, false, 1)]);
    let stroke = exported(&one_box(CYAN, |b| {
        stroked(b);
        b.stroke.overprint = true;
    }));
    assert!(overprint_states(&stroke).iter().all(|(_, s, opm)| *s && *opm == 1), "{:?}", overprint_states(&stroke));
    assert!(!overprint_states(&stroke).is_empty());
    let gap = exported(&one_box(CYAN, |b| {
        stroked(b);
        b.stroke.kind = designcraft_doc::StrokeType::Dashed { pattern: vec![12.0, 6.0] };
        b.stroke.gap_swatch = "C=0 M=0 Y=100 K=0".into();
        b.stroke.gap_overprint = true;
    }));
    assert!(overprint_states(&gap).iter().all(|(_, s, opm)| *s && *opm == 1), "{:?}", overprint_states(&gap));
    assert_eq!(overprint_states(&gap).len(), 1, "only the gap overprints");
    // PDF/A exports are RGB: overprint is left out, with a warning.
    let opts = PdfOptions { standard: crate::Standard::PdfA2b, ..PdfOptions::default() };
    let r = crate::export_pdf_with_report(&one_box(CYAN, |b| b.fill.overprint = true), &Cache::new(), &opts).unwrap();
    assert_eq!(overprint_states(&r.bytes), []);
    assert!(r.warnings.iter().any(|w| w == "PDF/A: overprint was left out"), "{:?}", r.warnings);
}

/// 100% [Black] fills, strokes and text overprint when Appearance of Black says so; tints of
/// it and other colors don't.
#[test]
fn black_overprints_with_the_preference() {
    let black = designcraft_color::swatch::BLACK;
    let mut on = one_box(black, |_| {});
    on.settings.overprint_black = true;
    assert_eq!(overprint_states(&exported(&on)), [(true, false, 1)]);
    let mut tint = one_box(black, |b| b.fill.tint = 0.5);
    tint.settings.overprint_black = true;
    assert_eq!(overprint_states(&exported(&tint)), []);
    assert_eq!(overprint_states(&exported(&one_box(black, |_| {}))), [], "off in the preferences");
    let mut stroke = one_box(designcraft_color::swatch::NONE, |b| {
        b.stroke.swatch = black.into();
        b.stroke.weight = 2.0;
    });
    stroke.settings.overprint_black = true;
    assert!(overprint_states(&exported(&stroke)).iter().any(|(_, s, opm)| *s && *opm == 1));
    // Text in [Black], the default.
    let mut text = Document::new(&NewDocument::default());
    let lid = text.default_layer();
    text.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 100.0), lid, "Black text", Default::default()).unwrap();
    assert_eq!(overprint_states(&exported(&text)), [(true, false, 1)]);
    // PDF/A leaves [Black] overprint out without a warning: it is the default for all black text.
    let opts = PdfOptions { standard: crate::Standard::PdfA2b, ..PdfOptions::default() };
    let r = crate::export_pdf_with_report(&text, &Cache::new(), &opts).unwrap();
    assert_eq!(overprint_states(&r.bytes), []);
    assert!(!r.warnings.iter().any(|w| w.contains("overprint")), "{:?}", r.warnings);
    text.settings.overprint_black = false;
    assert_eq!(overprint_states(&exported(&text)), []);
}

/// Ruby with Overprint Fill on overprints.
#[test]
fn ruby_overprint_fill_is_written() {
    use designcraft_doc::cjk_settings::AdornmentOverprint;
    let mut d = Document::new(&NewDocument::default());
    d.settings.overprint_black = false;
    let lid = d.default_layer();
    d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 576.0, 300.0), lid, "漢字です", Default::default()).unwrap();
    assert_eq!(overprint_states(&exported(&d)), []);
    let sid = *d.stories.keys().next().unwrap();
    d.story_mut(sid).unwrap().format_chars(0.."漢字".len(), |f| {
        f.over.ruby = Some("かんじ".into());
        f.over.ruby_overprint_fill = Some(AdornmentOverprint::On);
    });
    let r = crate::export_pdf_with_report(&d, &Cache::new(), &PdfOptions::default()).unwrap();
    assert_eq!(overprint_states(&r.bytes), [(true, false, 1)]);
    assert!(!r.warnings.iter().any(|w| w.contains("overprint")), "{:?}", r.warnings);
}

/// Knockout Group writes `/K true` and Isolate Blending `/I true`, each on its own.
#[test]
fn knockout_and_isolation_are_group_attributes() {
    let grouped = |knockout: bool, isolate: bool| {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let child = |d: &mut Document, r: Rect, swatch: &str| {
            let mut b = Item::new(ItemId(d.alloc()), lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(r));
            b.fill = Fill::swatch(swatch);
            std::sync::Arc::new(b)
        };
        let items = vec![
            child(&mut d, Rect::new(36.0, 36.0, 136.0, 136.0), "C=100 M=0 Y=0 K=0"),
            child(&mut d, Rect::new(86.0, 86.0, 186.0, 186.0), "C=0 M=100 Y=0 K=0"),
        ];
        let mut g = Item::new(
            ItemId(d.alloc()),
            lid,
            designcraft_doc::Shape::Group,
            designcraft_geom::shapes::rectangle(Rect::new(36.0, 36.0, 186.0, 186.0)),
        );
        g.content = designcraft_doc::Content::Group { items };
        g.knockout = knockout;
        g.isolate = isolate;
        d.insert_item(SpreadRef::Doc(0), g, None).unwrap();
        form_groups(&exported(&d))
    };
    let attributes =
        |groups: Vec<(Option<String>, bool, bool)>| groups.into_iter().map(|(_, i, k)| (i, k)).filter(|ik| *ik != (false, false)).collect::<Vec<_>>();
    assert_eq!(attributes(grouped(true, false)), [(false, true)]);
    assert_eq!(attributes(grouped(false, true)), [(true, false)]);
    assert_eq!(attributes(grouped(true, true)), [(true, true)]);
    assert_eq!(attributes(grouped(false, false)), []);
}
