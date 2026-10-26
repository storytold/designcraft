use super::support::{catalog, document, nameless_fixture, painted, properties_fixture};
use designcraft_compose::Cache;
use designcraft_engine::{Session, cmd::base64_encode};
use designcraft_pdf::{PdfOptions, Standard, export_pdf, export_pdf_with_report};
use serde_json::json;

const CASES: &[&str] = &[
    "/OCGs[5 0 R 6 3 R]",
    "/OCGs[5 0 R 6 3 R]/D null",
    "/OCGs[5 0 R 6 3 R]/D 42",
    "/OCGs[5 0 R 6 3 R]/D 99 0 R",
    "/OCGs[5 0 R 6 3 R]/D<</BaseState/Unchanged/OFF[6 3 R]>>",
    "/OCGs[5 0 R 6 3 R]/D<</BaseState/Other/OFF[6 3 R]>>",
    "/OCGs[5 0 R 6 3 R]/D<</BaseState 42/OFF[6 3 R]>>",
    "/OCGs[5 0 R 6 3 R]/D<</BaseState/ON/ON 42>>",
    "/OCGs[5 0 R 6 3 R]/D<</BaseState/ON/ON[null 5 0 R]>>",
];

fn incomplete_inventory(standard: &str, off: bool) {
    let cases: &[&str] = if off {
        &["/OCGs[5 0 R]/D<</BaseState/ON/OFF[6 3 R]>>", "/D<</ON[5 0 R]/OFF[6 3 R]>>"]
    } else {
        &["/OCGs[6 3 R]/D<</BaseState/OFF/ON[5 0 R]>>"]
    };
    for properties in cases {
        let source = properties_fixture(&format!("/OCProperties<<{properties}>>"), "Hidden");
        let mut session = Session::new();
        session.execute("file.new", &json!({"width": 100, "height": 100, "pages": 1, "facingPages": false, "margins": 0})).unwrap();
        session.execute("file.place", &json!({"base64": base64_encode(&source), "name": "incomplete.pdf", "x": 0, "y": 0, "width": 100})).unwrap();

        let result = session.execute("file.exportPdf", &json!({"standard": standard})).unwrap();

        let bytes = designcraft_engine::cmd::base64_decode(result["base64"].as_str().unwrap());
        assert_eq!(catalog(&bytes), [("Hidden".into(), true), ("Shown".into(), false)]);
        painted(&bytes, &[true, false]);
        super::assert_vector(&bytes);
        let warnings = result["warnings"].as_array().unwrap();
        assert_eq!(warnings.len(), 1);
        let warning = warnings[0].as_str().unwrap();
        assert!(warning.contains("incomplete.pdf") && warning.contains("layer list incomplete"), "{warning}");
        assert!(!warning.contains("layer state not readable"), "{warning}");
        if standard == "x4" {
            assert!(designcraft_pdf::check_pdfx4(&bytes).is_empty());
        }
    }
}

#[test]
fn plain_incomplete_inventory_preserves_off() {
    incomplete_inventory("none", true);
}

#[test]
fn pdfx_incomplete_inventory_preserves_off() {
    incomplete_inventory("x4", true);
}

#[test]
fn plain_incomplete_inventory_preserves_on() {
    incomplete_inventory("none", false);
}

#[test]
fn pdfx_incomplete_inventory_preserves_on() {
    incomplete_inventory("x4", false);
}

#[test]
fn incomplete_inventory_reports_both_warnings_during_fallback() {
    for standard in [Standard::None, Standard::PdfX4] {
        let source = properties_fixture("/OCProperties<</OCGs[5 0 R]/D<</BaseState/Unchanged/OFF[6 3 R]>>>>", "Hidden");
        let doc = document(source, &[&[]]);

        let report = export_pdf_with_report(&doc, &Cache::new(), &PdfOptions { standard, ..Default::default() }).unwrap();

        assert_eq!(report.warnings.len(), 1);
        let warning = &report.warnings[0];
        assert!(
            warning.contains("layers.pdf") && warning.contains("layer list incomplete") && warning.contains("layer state not readable"),
            "{warning}"
        );
        painted(&report.bytes, &[true, true]);
        if standard == Standard::PdfX4 {
            assert!(designcraft_pdf::check_pdfx4(&report.bytes).is_empty());
        }
    }
}

fn visible_recovery(source: Vec<u8>, standard: &str) {
    let mut session = Session::new();
    session.execute("file.new", &json!({"width": 100, "height": 100, "pages": 1, "facingPages": false, "margins": 0})).unwrap();
    session.execute("file.place", &json!({"base64": base64_encode(&source), "name": "visible-recovery.pdf", "x": 0, "y": 0, "width": 100})).unwrap();

    let result = session.execute("file.exportPdf", &json!({"standard": standard})).unwrap();

    let warnings = result["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].as_str().unwrap().contains("visible-recovery.pdf"));
    let bytes = designcraft_engine::cmd::base64_decode(result["base64"].as_str().unwrap());
    let groups = catalog(&bytes);
    assert_eq!(groups.len(), 2);
    assert!(groups.iter().all(|(name, hidden)| !name.is_empty() && !hidden));
    super::assert_vector(&bytes);
    painted(&bytes, &[true, true]);
    if standard == "x4" {
        assert!(designcraft_pdf::check_pdfx4(&bytes).is_empty());
    }
}

#[test]
fn plain_export_recovers_malformed_on_when_defaults_are_visible() {
    visible_recovery(super::support::fixture("/BaseState/ON/ON 42", "Hidden"), "none");
}

#[test]
fn pdfx_export_recovers_malformed_on_when_defaults_are_visible() {
    visible_recovery(super::support::fixture("/BaseState/ON/ON 42", "Hidden"), "x4");
}

#[test]
fn plain_export_names_visible_group_when_display_name_is_missing() {
    visible_recovery(nameless_fixture("/BaseState/ON"), "none");
}

#[test]
fn pdfx_export_names_visible_group_when_display_name_is_missing() {
    visible_recovery(nameless_fixture("/BaseState/ON"), "x4");
}

#[test]
fn missing_display_name_fails_when_an_override_could_match() {
    for standard in [Standard::None, Standard::PdfX4] {
        let doc = document(nameless_fixture("/BaseState/ON"), &[&["Shown"]]);

        let result = export_pdf(&doc, &Cache::new(), &PdfOptions { standard, ..Default::default() });

        assert!(result.is_err(), "a nameless group could match the explicit hidden name");
    }
}

#[test]
fn malformed_on_fails_when_base_state_can_hide_groups() {
    for standard in [Standard::None, Standard::PdfX4] {
        let doc = document(super::support::fixture("/BaseState/OFF/ON 42", "Hidden"), &[&[]]);

        let result = export_pdf(&doc, &Cache::new(), &PdfOptions { standard, ..Default::default() });

        assert!(result.is_err(), "all-ON recovery would expose groups hidden by BaseState OFF");
    }
}

#[test]
fn name_repair_preserves_readable_default_off_state() {
    for standard in [Standard::None, Standard::PdfX4] {
        let doc = document(nameless_fixture("/BaseState/ON/OFF[6 3 R]"), &[&[]]);

        let report = export_pdf_with_report(&doc, &Cache::new(), &PdfOptions { standard, ..Default::default() }).unwrap();

        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("layers.pdf"));
        assert_eq!(catalog(&report.bytes).iter().filter(|(_, hidden)| *hidden).count(), 1);
        painted(&report.bytes, &[true, false]);
        if standard == Standard::PdfX4 {
            assert!(designcraft_pdf::check_pdfx4(&report.bytes).is_empty());
        }
    }
}

fn fallback_exports(standard: Standard) {
    let mut failures = Vec::new();
    for properties in CASES {
        let source = properties_fixture(&format!("/OCProperties<<{properties}>>"), "Hidden");
        let doc = document(source, &[&[], &["Shown"]]);

        let report = match export_pdf_with_report(&doc, &Cache::new(), &PdfOptions { standard, created: Some(0), ..Default::default() }) {
            Ok(report) => report,
            Err(error) => {
                failures.push(format!("{properties}: {error}"));
                continue;
            }
        };

        if report.warnings.len() != 1 {
            failures.push(format!("{properties}: expected one fallback warning, got {:?}", report.warnings));
            continue;
        }
        assert!(report.warnings[0].contains("layers.pdf"));
        assert_eq!(catalog(&report.bytes), [("Hidden".into(), false), ("Hidden".into(), false), ("Shown".into(), false), ("Shown".into(), true)]);
        super::assert_vector(&report.bytes);
        painted(&report.bytes, &[true, true, false, true]);
        if standard == Standard::PdfX4 {
            assert!(designcraft_pdf::check_pdfx4(&report.bytes).is_empty());
        }
    }
    assert!(failures.is_empty(), "fallback export failures: {failures:#?}");
}

#[test]
fn plain_fallback_shows_all_layers_and_preserves_frame_overrides() {
    fallback_exports(Standard::None);
}

#[test]
fn pdfx_fallback_shows_all_layers_and_preserves_frame_overrides() {
    fallback_exports(Standard::PdfX4);
}

#[test]
fn file_export_reports_fallback_with_filename_and_preserves_overrides() {
    for properties in CASES {
        for standard in ["none", "x4"] {
            for hidden in [false, true] {
                let source = properties_fixture(&format!("/OCProperties<<{properties}>>"), "Hidden");
                let mut session = Session::new();
                session.execute("file.new", &json!({"width": 100, "height": 100, "pages": 1, "facingPages": false, "margins": 0})).unwrap();
                session
                    .execute("file.place", &json!({"base64": base64_encode(&source), "name": "fallback.pdf", "x": 0, "y": 0, "width": 100}))
                    .unwrap();
                if hidden {
                    session.execute("object.layerOptions", &json!({"hidden": ["Shown"]})).unwrap();
                }

                let result = session.execute("file.exportPdf", &json!({"standard": standard})).unwrap();

                let warnings = result["warnings"].as_array().unwrap();
                assert_eq!(warnings.len(), 1, "{properties}: {result}");
                assert!(warnings[0].as_str().unwrap().contains("fallback.pdf"));
                let bytes = designcraft_engine::cmd::base64_decode(result["base64"].as_str().unwrap());
                assert_eq!(catalog(&bytes), [("Hidden".into(), false), ("Shown".into(), hidden)]);
                painted(&bytes, &[!hidden, true]);
            }
        }
    }
}

#[test]
fn contradictory_and_unresolved_states_fail_even_with_fallback_or_overrides() {
    for properties in [
        "/OCGs[5 0 R 6 3 R]/D<</ON[6 3 R]/OFF[6 3 R]>>",
        "/OCGs[5 0 R]/D<</BaseState/Unchanged/ON[6 3 R]/OFF[6 3 R]>>",
        "/OCGs[5 0 R 6 3 R]/D<</BaseState/Unchanged/OFF[99 0 R]>>",
        "/OCGs[5 0 R 6 3 R]/D<</ON[99 0 R]>>",
        "/OCGs[5 0 R 6 3 R 99 0 R]",
        "/OCGs[5 0 R 6 3 R]/D<</OFF[2 0 R]>>",
        "/OCGs[5 0 R 6 3 R]/D<</OFF[6 0 R]>>",
        "/OCGs[5 0 R 6 3 R]/D<</OFF[null]>>",
        "/OCGs[5 0 R 6 3 R]/D<</OFF 42>>",
        "/OCGs[5 0 R 6 3 R]/D<</OFF 99 0 R>>",
        "/OCGs[5 0 R 6 3 R]/D<</BaseState/ON/ON[null 99 0 R]>>",
        "/OCGs[5 0 R 6 3 R]/D<</BaseState/ON/ON 99 0 R>>",
        "/OCGs[5 0 R 6 3 R]/D<</BaseState/ON/ON 42/OFF[6 3 R]>>",
    ] {
        for standard in [Standard::None, Standard::PdfX4] {
            for hidden in [&[][..], &["Shown"][..]] {
                let doc = document(properties_fixture(&format!("/OCProperties<<{properties}>>"), "Hidden"), &[hidden]);

                let result = export_pdf(&doc, &Cache::new(), &PdfOptions { standard, ..Default::default() });

                assert!(result.is_err(), "unsafe state accepted: {properties}");
            }
        }
    }
}
