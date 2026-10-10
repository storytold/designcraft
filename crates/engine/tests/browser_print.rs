//! Browser Print must reject native spooler operations without trapping the editor.
#![cfg(target_arch = "wasm32")]

use designcraft_engine::Session;
use serde_json::json;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

fn print_is_safe(dry_run: bool) {
    let mut session = Session::new();
    session.execute("file.new", &json!({"pages": 1})).unwrap();
    let error = session.execute("file.print", &json!({"dryRun": dry_run})).unwrap_err().to_string();
    assert!(error.contains("printing isn't available on the web"), "{error}");
    assert!(error.contains("export a PDF"), "{error}");
    session.execute("frame.create", &json!({"rect": [36, 36, 100, 100], "content": "text"})).unwrap();
    let exported = session.execute("file.exportPdf", &json!({})).unwrap();
    assert!(exported["bytes"].as_u64().unwrap() > 500);
    assert_eq!(exported["pages"], 1);
    assert!(session.execute("file.print", &json!({"pages": "9", "dryRun": dry_run})).is_err());
}

#[wasm_bindgen_test]
fn print_confirmation_preserves_editing_and_pdf_export() {
    print_is_safe(false);
}

#[wasm_bindgen_test]
fn print_dry_run_is_also_unsupported_without_native_operations() {
    print_is_safe(true);
}
