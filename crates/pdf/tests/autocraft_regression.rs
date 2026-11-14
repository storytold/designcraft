//! Tagged PDF export: a link annotation on text is in the structure tree, in its paragraph.

use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Document, Hyperlink, HyperlinkDest, HyperlinkSource, ParaFormat, SpreadRef};
use designcraft_geom::Rect;
use designcraft_pdf::{PdfOptions, export_pdf};

/// The PDF's text with all whitespace removed, so that keys and values read as `/S/Link`.
fn squeezed(tagged: bool) -> String {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let text = "Visit example site";
    let (_, story) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(50.0, 50.0, 400.0, 300.0), lid, text, ParaFormat::default()).unwrap();
    d.hyperlinks.push(Hyperlink {
        id: 1,
        name: "example".into(),
        source: HyperlinkSource::Text { story, start: 6, end: 13 },
        dest: HyperlinkDest::Url("https://example.com".into()),
    });
    let bytes = export_pdf(&d, &Cache::new(), &PdfOptions { tagged, compress: false, ..PdfOptions::default() }).unwrap();
    String::from_utf8_lossy(&bytes).chars().filter(|c| !c.is_ascii_whitespace()).collect()
}

#[test]
fn tagged_export_puts_the_link_annotation_in_the_structure_tree() {
    let pdf = squeezed(true);
    assert_eq!(pdf.matches("/Subtype/Link").count(), 1, "one link annotation");
    let parents = pdf.match_indices("/StructParent").filter(|(at, key)| pdf[at + key.len()..].starts_with(|c: char| c.is_ascii_digit())).count();
    assert_eq!(parents, 1, "the annotation has a /StructParent");
    assert_eq!(pdf.matches("/S/Link").count(), 1, "one /Link structure element");
    assert_eq!(pdf.matches("/Type/OBJR").count(), 1, "the link element points at the annotation");
}

#[test]
fn untagged_export_has_no_link_structure() {
    let pdf = squeezed(false);
    assert_eq!(pdf.matches("/Subtype/Link").count(), 1);
    assert!(!pdf.contains("/OBJR") && !pdf.contains("/S/Link"));
}
