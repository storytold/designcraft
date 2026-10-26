use std::time::{Duration, Instant};

use super::{DEPTH_WARNING, FontScan, LIMIT_WARNING, MAX_OPERATIONS, unembedded_fonts};
use crate::check_pdfx4;
use crate::pdfx::tests::{place, tiny_pdf_with, x4};
use designcraft_doc::{Document, build::NewDocument};

fn stream(entries: &str, content: &str) -> String {
    format!("<<{entries}/Length {}>>\nstream\n{content}\nendstream", content.len())
}

fn fanout_pdf(levels: usize) -> Vec<u8> {
    let mut objects = vec![
        "<</Type/Catalog/Pages 2 0 R>>".to_string(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Resources<</XObject<</Next 5 0 R>>>>/Contents 4 0 R>>".to_string(),
        stream("", "/Next Do"),
    ];
    for level in 0..levels {
        let resources = if level + 1 == levels {
            "/Font<</F1<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>>>".to_string()
        } else {
            format!("/XObject<</Next {} 0 R>>", level + 6)
        };
        let content = if level + 1 == levels { "BT /F1 12 Tf (bottom) Tj ET".to_string() } else { "/Next Do\n".repeat(10) };
        objects.push(stream(&format!("/Type/XObject/Subtype/Form/BBox[0 0 100 100]/Resources<<{resources}>>"), &content));
    }
    pdf_objects(&objects)
}

fn pdf_objects(objects: &[String]) -> Vec<u8> {
    let mut bytes = b"%PDF-1.6\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f\r\n", objects.len() + 1).as_bytes());
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n\r\n").as_bytes());
    }
    bytes.extend_from_slice(format!("trailer\n<</Size {}/Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
    bytes
}

#[test]
fn nested_form_fanout_is_memoized_and_reports_bottom_font() {
    let bytes = fanout_pdf(16);
    let pdf = hayro_syntax::Pdf::new(bytes.clone()).unwrap();
    let page = pdf.pages().first().unwrap();
    let mut scan = FontScan::default();
    let start = Instant::now();
    scan.content(page.page_stream().unwrap(), page.resources(), 0);
    assert!(start.elapsed() < Duration::from_millis(500));
    assert_eq!(scan.operations, 155);
    assert_eq!(scan.completed.len(), 16);
    assert!(!scan.limited);
    assert!(!scan.depth_limited);
    assert_eq!(scan.found.into_iter().collect::<Vec<_>>(), ["Helvetica"]);
    assert!(check_pdfx4(&bytes).iter().any(|w| w.contains("Helvetica isn't embedded")));
}

#[test]
fn placed_pdf_nested_form_fanout_reports_unembedded_font() {
    let mut doc = Document::new(&NewDocument::default());
    place(&mut doc, "fanout.pdf", fanout_pdf(14), (100.0, 100.0), (72.0, 72.0));
    let report = x4(&doc);
    assert!(report.warnings.iter().any(|w| w.contains("Helvetica isn't embedded")), "{:?}", report.warnings);
}

#[test]
fn form_memo_resolves_resource_names_in_each_parent_context() {
    let wrapper = |font: &str| {
        stream(
            &format!("/Type/XObject/Subtype/Form/BBox[0 0 100 100]/Resources<</Font<</F1<</Type/Font/Subtype/Type1/BaseFont/{font}>>>>>>"),
            "/Shared Do",
        )
    };
    let bytes = pdf_objects(&[
        "<</Type/Catalog/Pages 2 0 R>>".into(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".into(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Resources<</XObject<</A 5 0 R/B 6 0 R/Shared 7 0 R>>>>/Contents 4 0 R>>".into(),
        stream("", "/A Do /B Do"),
        wrapper("Helvetica"),
        wrapper("Times-Roman"),
        stream("/Type/XObject/Subtype/Form/BBox[0 0 100 100]", "BT /F1 12 Tf (shared) Tj ET"),
    ]);
    let issues = unembedded_fonts(&bytes);
    assert_eq!(issues.len(), 2, "{issues:?}");
    assert!(issues.iter().any(|w| w.contains("Helvetica isn't embedded")));
    assert!(issues.iter().any(|w| w.contains("Times-Roman isn't embedded")));
}

#[test]
fn form_memo_replays_font_findings_on_each_page() {
    let page = "<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Resources<</XObject<</Shared 6 0 R>>>>/Contents 5 0 R>>";
    let bytes = pdf_objects(&[
        "<</Type/Catalog/Pages 2 0 R>>".into(),
        "<</Type/Pages/Kids[3 0 R 4 0 R]/Count 2>>".into(),
        page.into(),
        page.into(),
        stream("", "/Shared Do"),
        stream(
            "/Type/XObject/Subtype/Form/BBox[0 0 100 100]/Resources<</Font<</F1<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>>>>>",
            "BT /F1 12 Tf (shared) Tj ET",
        ),
    ]);
    let issues = unembedded_fonts(&bytes);
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert!(issues[0].contains("(pages 1, 2)"), "{issues:?}");
}

#[test]
fn form_memo_keeps_inherited_font_and_resource_contexts_distinct() {
    let draw = "BT (inherited) Tj /F1 12 Tf (resource) Tj ET";
    let form = stream("/Type/XObject/Subtype/Form/BBox[0 0 100 100]", draw);
    let entries = "/Resources<</Font<</F1<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>/F2<</Type/Font/Subtype/Type1/BaseFont/Times-Roman>>>>/XObject<</X1 6 0 R>>>>";
    let bytes = tiny_pdf_with(entries, "/F1 12 Tf /X1 Do /F2 12 Tf /X1 Do", &form);
    let issues = unembedded_fonts(&bytes);
    assert_eq!(issues.len(), 2, "{issues:?}");
    assert!(issues.iter().any(|w| w.contains("Times-Roman isn't embedded")));
}

#[test]
fn font_scan_operation_budget_warns_instead_of_silently_truncating() {
    let content = "q Q\n".repeat(MAX_OPERATIONS);
    let bytes = tiny_pdf_with("/Resources<<>>", &content, "<<>>");
    assert_eq!(unembedded_fonts(&bytes), [LIMIT_WARNING]);
}

#[test]
fn font_scan_depth_limit_warns_instead_of_silently_truncating() {
    // The font is two levels below the deepest form followed, and every level draws the next
    // ten times: memo replays of the cut-short subtree still give one warning.
    assert_eq!(unembedded_fonts(&fanout_pdf(18)), [DEPTH_WARNING]);
}

#[test]
fn font_scan_cycles_are_not_reported_as_incomplete() {
    let font = "/Font<</F1<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>>>";
    let bytes = pdf_objects(&[
        "<</Type/Catalog/Pages 2 0 R>>".to_string(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Resources<</XObject<</A 5 0 R>>>>/Contents 4 0 R>>".to_string(),
        stream("", "/A Do"),
        stream(&format!("/Type/XObject/Subtype/Form/BBox[0 0 100 100]/Resources<<{font}/XObject<</B 6 0 R>>>>"), "BT /F1 12 Tf (a) Tj ET /B Do"),
        stream("/Type/XObject/Subtype/Form/BBox[0 0 100 100]/Resources<</XObject<</A 5 0 R/B 6 0 R>>>>", "/A Do /B Do"),
    ]);
    let warnings = unembedded_fonts(&bytes);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("Helvetica isn't embedded"), "{warnings:?}");
}
