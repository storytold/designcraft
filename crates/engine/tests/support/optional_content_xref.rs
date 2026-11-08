use super::support::{catalog, document, fixture, nameless_fixture, painted};
use designcraft_compose::Cache;
use designcraft_doc::{AssetId, Content};
use designcraft_pdf::{BookletKind, BookletOptions, PdfOptions, Standard, export_booklet, export_pdf, export_pdf_with_report, merge_pdfs};
use std::sync::Arc;

fn broken_xref(bytes: Vec<u8>) -> Vec<u8> {
    let text = String::from_utf8(bytes).unwrap();
    let (body, _) = text.rsplit_once("startxref").unwrap();
    let bytes = format!("{body}startxref\n1\n%%EOF\n").into_bytes();
    let pdf = hayro_syntax::Pdf::new(bytes.clone()).expect("hayro must repair the synthetic broken xref");
    assert_eq!(pdf.pages().len(), 1);
    let root = pdf.xref().get::<hayro_syntax::object::Dict<'_>>(pdf.xref().root_id()).unwrap();
    assert!(root.get::<hayro_syntax::object::Dict<'_>>(b"OCProperties").is_some());
    bytes
}

fn all_visible(standard: Standard) {
    let source = broken_xref(fixture("/BaseState/ON", "Hidden"));
    painted(&source, &[true, true]);
    let doc = document(source, &[&[]]);

    let report = export_pdf_with_report(&doc, &Cache::new(), &PdfOptions { standard, ..Default::default() }).unwrap();

    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(catalog(&report.bytes), [("Hidden".into(), false), ("Shown".into(), false)]);
    super::assert_vector(&report.bytes);
    painted(&report.bytes, &[true, true]);
    if standard == Standard::PdfX4 {
        assert!(designcraft_pdf::check_pdfx4(&report.bytes).is_empty());
    }
}

#[test]
fn plain_export_accepts_repaired_xref_when_all_layers_are_visible() {
    all_visible(Standard::None);
}

#[test]
fn pdfx_export_accepts_repaired_xref_when_all_layers_are_visible() {
    all_visible(Standard::PdfX4);
}

#[test]
fn booklet_accepts_repaired_xref_when_all_layers_are_visible() {
    let doc = document(broken_xref(fixture("/BaseState/ON", "Hidden")), &[&[]]);
    let options = BookletOptions { kind: BookletKind::TwoUpConsecutive, ..Default::default() };

    let report = export_booklet(&doc, &Cache::new(), &options).unwrap();

    assert!(report.warnings.is_empty());
    assert_eq!(catalog(&report.bytes), [("Hidden".into(), false), ("Shown".into(), false)]);
    super::assert_vector(&report.bytes);
    painted(&report.bytes, &[true, true, false, false]);
}

#[test]
fn book_merge_accepts_repaired_xref_when_all_layers_are_visible() {
    let source = broken_xref(fixture("/BaseState/ON", "Hidden"));

    let bytes = merge_pdfs(&[(source, vec![(100.0, 100.0)])], None).unwrap();

    assert_eq!(catalog(&bytes), [("Hidden".into(), false), ("Shown".into(), false)]);
    super::assert_vector(&bytes);
    painted(&bytes, &[true, true]);
}

#[test]
fn repaired_xref_with_default_off_fails_with_named_damage_error() {
    let doc = document(broken_xref(fixture("/BaseState/ON/OFF[6 3 R]", "Hidden")), &[&[]]);

    for standard in [Standard::None, Standard::PdfX4] {
        let error = export_pdf(&doc, &Cache::new(), &PdfOptions { standard, ..Default::default() }).unwrap_err().to_string();
        assert!(error.contains("layers.pdf"), "{error}");
        assert!(error.contains("cross-reference table is damaged"), "{error}");
        assert!(!error.contains("optional-content state"), "{error}");
    }
}

#[test]
fn repaired_xref_with_explicit_hidden_layer_fails_with_named_damage_error() {
    let doc = document(broken_xref(fixture("/BaseState/ON", "Hidden")), &[&["Hidden"]]);

    let error = export_pdf(&doc, &Cache::new(), &PdfOptions::default()).unwrap_err().to_string();

    assert!(error.contains("layers.pdf") && error.contains("cross-reference table is damaged"), "{error}");
}

#[test]
fn repaired_xref_needing_generated_name_fails_with_named_damage_error() {
    let doc = document(broken_xref(nameless_fixture("/BaseState/ON")), &[&[]]);

    let error = export_pdf(&doc, &Cache::new(), &PdfOptions::default()).unwrap_err().to_string();

    assert!(error.contains("layers.pdf") && error.contains("cross-reference table is damaged"), "{error}");
}

fn two_assets(first: Vec<u8>, second: Vec<u8>) -> designcraft_doc::Document {
    let mut doc = document(first, &[&[], &[]]);
    let original = doc.assets.values().next().unwrap().as_ref().clone();
    Arc::make_mut(doc.assets.values_mut().next().unwrap()).name = "first.pdf".into();
    let id = AssetId(doc.alloc());
    let mut other = original;
    other.id = id;
    other.name = "second.pdf".into();
    other.data = Arc::new(second);
    doc.add_asset(other);
    let spread = Arc::make_mut(doc.spreads.first_mut().unwrap());
    let item = Arc::make_mut(spread.items.last_mut().unwrap());
    let Content::Graphic(graphic) = &mut item.content else { panic!("fixture must have a graphic") };
    graphic.asset = id;
    doc
}

#[test]
fn error_identifies_unresolvable_pdf_among_two_placements() {
    let doc = two_assets(fixture("/BaseState/ON", "Hidden"), fixture("/OFF[99 0 R]", "Hidden"));

    let error = export_pdf(&doc, &Cache::new(), &PdfOptions::default()).unwrap_err().to_string();

    assert_eq!(error, "PDF writer error: second.pdf: the placed PDF's optional-content state could not be preserved safely");
}

#[test]
fn first_placement_error_is_not_overwritten() {
    let doc = two_assets(fixture("/OFF[99 0 R]", "Hidden"), fixture("/OFF[98 0 R]", "Hidden"));

    let error = export_pdf(&doc, &Cache::new(), &PdfOptions::default()).unwrap_err().to_string();

    assert!(error.contains("first.pdf"), "{error}");
    assert!(!error.contains("second.pdf"), "{error}");
}

#[test]
fn repeated_recoverable_variant_reports_one_warning_and_paints_each_placement() {
    let doc = document(fixture("/BaseState/Unchanged", "Hidden"), &[&[], &[]]);

    let report = export_pdf_with_report(&doc, &Cache::new(), &PdfOptions::default()).unwrap();

    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("layers.pdf"));
    painted(&report.bytes, &[true, true, true, true]);
}
