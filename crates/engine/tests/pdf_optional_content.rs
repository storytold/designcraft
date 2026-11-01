#[path = "support/optional_content.rs"]
mod support;

#[path = "support/optional_content_fallback.rs"]
mod fallback;

#[path = "support/optional_content_xref.rs"]
mod xref;

use designcraft_compose::Cache;
use designcraft_pdf::{PdfOptions, Standard, export_pdf, export_pdf_with_report};
use support::{catalog, document, fixture, painted};

const DEFAULT: &str = "/BaseState/ON/OFF[6 3 R]";
const USAGE_APPLICATIONS: &str = "/BaseState/ON/ON[5 0 R]/OFF[6 3 R]/AS[<</Event/View/Category[/View]/OCGs[5 0 R 6 3 R]>><</Event/Print/Category[/Print]/OCGs[5 0 R 6 3 R]>><</Event/Export/Category[/Export]/OCGs[5 0 R 6 3 R]>>]";

#[test]
fn source_layer_dictionaries_are_readable() {
    let pdf = hayro_syntax::Pdf::new(fixture(DEFAULT, "Hidden")).unwrap();
    for id in [(5, 0), (6, 3)] {
        assert!(pdf.xref().get::<hayro_syntax::object::Dict<'_>>(hayro_syntax::object::ObjectIdentifier::new(id.0, id.1)).is_some(), "{id:?}");
    }
}

fn exported(config: &str, standard: Standard) -> Vec<u8> {
    let doc = document(fixture(config, "Hidden"), &[&[]]);
    let report = export_pdf_with_report(&doc, &Cache::new(), &PdfOptions { standard, created: Some(0), ..Default::default() }).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    report.bytes
}

#[test]
fn source_default_off_renders_before_export() {
    let bytes = fixture(DEFAULT, "Hidden");

    painted(&bytes, &[true, false]);
}

#[test]
fn plain_export_registers_default_off_layers() {
    let bytes = exported(DEFAULT, Standard::None);

    assert_eq!(catalog(&bytes), [("Hidden".into(), true), ("Shown".into(), false)]);
}

#[test]
fn pdfx_export_preserves_default_off_catalog() {
    let bytes = exported(DEFAULT, Standard::PdfX4);

    assert_eq!(catalog(&bytes), [("Hidden".into(), true), ("Shown".into(), false)]);
    assert!(designcraft_pdf::check_pdfx4(&bytes).is_empty());
}

#[test]
fn plain_export_paints_only_the_shown_layer() {
    let bytes = exported(DEFAULT, Standard::None);

    painted(&bytes, &[true, false]);
}

#[test]
fn pdfx_export_paints_only_the_shown_layer() {
    let bytes = exported(DEFAULT, Standard::PdfX4);

    painted(&bytes, &[true, false]);
}

#[test]
fn plain_export_preserves_defaults_with_usage_applications() {
    let bytes = exported(USAGE_APPLICATIONS, Standard::None);

    assert_eq!(catalog(&bytes), [("Hidden".into(), true), ("Shown".into(), false)]);
    assert_vector(&bytes);
    painted(&bytes, &[true, false]);
}

#[test]
fn pdfx_export_preserves_defaults_with_usage_applications() {
    let bytes = exported(USAGE_APPLICATIONS, Standard::PdfX4);

    assert_eq!(catalog(&bytes), [("Hidden".into(), true), ("Shown".into(), false)]);
    assert!(designcraft_pdf::check_pdfx4(&bytes).is_empty());
    assert_vector(&bytes);
    painted(&bytes, &[true, false]);
}

#[test]
fn plain_export_draws_groups_without_catalog_properties() {
    let doc = document(support::orphaned_fixture(), &[&[]]);

    let bytes = export_pdf(&doc, &Cache::new(), &PdfOptions::default()).unwrap();

    assert_eq!(catalog(&bytes), [("Hidden".into(), false), ("Shown".into(), false)]);
    assert_vector(&bytes);
    painted(&bytes, &[true, true]);
}

#[test]
fn pdfx_export_draws_groups_without_catalog_properties() {
    let doc = document(support::orphaned_fixture(), &[&[]]);

    let bytes = export_pdf(&doc, &Cache::new(), &PdfOptions { standard: Standard::PdfX4, ..Default::default() }).unwrap();

    assert_eq!(catalog(&bytes), [("Hidden".into(), false), ("Shown".into(), false)]);
    assert!(designcraft_pdf::check_pdfx4(&bytes).is_empty());
    assert_vector(&bytes);
    painted(&bytes, &[true, true]);
}

#[test]
fn base_off_with_on_exception_is_preserved() {
    let bytes = exported("/BaseState/OFF/ON[5 0 R]", Standard::None);

    assert_eq!(catalog(&bytes), [("Hidden".into(), true), ("Shown".into(), false)]);
    painted(&bytes, &[true, false]);
}

#[test]
fn same_asset_placements_keep_opposing_layer_states() {
    let doc = document(fixture(DEFAULT, "Hidden"), &[&[], &["Shown"]]);

    let bytes = export_pdf(&doc, &Cache::new(), &PdfOptions::default()).unwrap();

    assert_eq!(catalog(&bytes), [("Hidden".into(), false), ("Hidden".into(), true), ("Shown".into(), false), ("Shown".into(), true)]);
    painted(&bytes, &[true, false, false, true]);
}

#[test]
fn duplicate_names_with_distinct_generations_keep_default_states() {
    let doc = document(fixture(DEFAULT, "Shown"), &[&[]]);

    let bytes = export_pdf(&doc, &Cache::new(), &PdfOptions::default()).unwrap();

    assert_eq!(catalog(&bytes), [("Shown".into(), false), ("Shown".into(), true)]);
    painted(&bytes, &[true, false]);
}

#[test]
fn unresolved_default_off_reference_fails_export() {
    let doc = document(fixture("/OFF[99 0 R]", "Hidden"), &[&[]]);

    let result = export_pdf(&doc, &Cache::new(), &PdfOptions::default());

    assert!(result.is_err(), "an unreadable layer state must not print hidden artwork");
}

#[test]
fn unlayered_pdf_stays_vector_without_optional_content() {
    let doc = document(support::unlayered_fixture(), &[&[]]);

    let bytes = export_pdf(&doc, &Cache::new(), &PdfOptions::default()).unwrap();

    let pdf = hayro_syntax::Pdf::new(bytes.clone()).unwrap();
    let root = pdf.xref().get::<hayro_syntax::object::Dict<'_>>(pdf.xref().root_id()).unwrap();
    assert!(!root.contains_key(b"OCProperties"));
    assert_vector(&bytes);
    painted(&bytes, &[true, false]);
}

fn assert_vector(bytes: &[u8]) {
    use hayro_syntax::object::{Name, Object};
    let pdf = hayro_syntax::Pdf::new(bytes.to_vec()).unwrap();
    for object in pdf.objects() {
        if let Object::Stream(stream) = object {
            assert_ne!(stream.dict().get::<Name<'_>>(b"Subtype").as_ref().map(AsRef::as_ref), Some(b"Image".as_slice()));
        }
    }
}

#[test]
fn layer_options_command_preserves_vector_overrides_in_plain_and_pdfx() {
    use designcraft_engine::{
        Session,
        cmd::{base64_decode, base64_encode},
    };
    use serde_json::json;
    for standard in ["none", "x4"] {
        let mut session = Session::new();
        session.execute("file.new", &json!({"width": 100, "height": 100, "pages": 1, "facingPages": false, "margins": 0})).unwrap();
        session
            .execute("file.place", &json!({"base64": base64_encode(&fixture(DEFAULT, "Hidden")), "name": "layers.pdf", "x": 0, "y": 0, "width": 100}))
            .unwrap();
        session.execute("object.layerOptions", &json!({"hidden": ["Shown"]})).unwrap();

        let result = session.execute("file.exportPdf", &json!({"standard": standard})).unwrap();

        let bytes = base64_decode(result["base64"].as_str().unwrap());
        assert_eq!(catalog(&bytes), [("Hidden".into(), false), ("Shown".into(), true)]);
        assert_vector(&bytes);
        painted(&bytes, &[false, true]);
    }
}

#[test]
fn exported_optional_content_survives_replacement_and_book_merge() {
    let first = exported(DEFAULT, Standard::None);
    let doc = document(first.clone(), &[&[]]);

    let replaced = export_pdf(&doc, &Cache::new(), &PdfOptions::default()).unwrap();
    let merged = designcraft_pdf::merge_pdfs(&[(first, vec![(100.0, 100.0)])], None).unwrap();

    for bytes in [replaced, merged] {
        assert_eq!(catalog(&bytes), [("Hidden".into(), true), ("Shown".into(), false)]);
        painted(&bytes, &[true, false]);
    }
}

#[test]
fn booklet_export_preserves_optional_content() {
    let doc = document(fixture(DEFAULT, "Hidden"), &[&[]]);
    let options = designcraft_pdf::BookletOptions { kind: designcraft_pdf::BookletKind::TwoUpConsecutive, ..Default::default() };

    let report = designcraft_pdf::export_booklet(&doc, &Cache::new(), &options).unwrap();

    assert!(report.warnings.is_empty());
    assert_eq!(catalog(&report.bytes), [("Hidden".into(), true), ("Shown".into(), false)]);
    assert_vector(&report.bytes);
    painted(&report.bytes, &[true, false, false, false]);
}

#[test]
fn archival_export_still_rejects_unvalidated_placed_pdf() {
    let doc = document(fixture(DEFAULT, "Hidden"), &[&[]]);

    let error = export_pdf(&doc, &Cache::new(), &PdfOptions { standard: Standard::PdfA2b, ..Default::default() }).unwrap_err();

    assert!(error.to_string().contains("EmbeddedPDF"), "{error}");
}

#[test]
fn source_xref_stream_preserves_layer_states() {
    let doc = document(support::stream_fixture(DEFAULT), &[&[]]);

    let bytes = export_pdf(&doc, &Cache::new(), &PdfOptions::default()).unwrap();

    assert_eq!(catalog(&bytes), [("Hidden".into(), true), ("Shown".into(), false)]);
    painted(&bytes, &[true, false]);
}
