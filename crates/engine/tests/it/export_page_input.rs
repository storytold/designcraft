//! Public export-command page input must be rejected before narrowing on 32-bit targets.
use designcraft_engine::{EngineError, Session};
use serde_json::json;

fn rejects(numbers: &[u64]) {
    let mut session = Session::new();
    session.execute("file.new", &json!({"pages": 1})).unwrap();
    let before = serde_json::to_value(&session.doc().unwrap().doc).unwrap();
    for command in ["file.exportPdf", "file.exportPptx"] {
        for &number in numbers {
            for pages in [json!(number), json!([number])] {
                #[cfg(target_arch = "wasm32")]
                wasm_bindgen_test::console_log!("SAFETY_REJECTION_INPUT {command} {pages}");
                #[cfg(not(target_arch = "wasm32"))]
                println!("SAFETY_REJECTION_INPUT {command} {pages}");
                match session.execute(command, &json!({"pages": pages})) {
                    Err(EngineError::BadParams { cmd, msg }) => {
                        assert_eq!(cmd, command);
                        assert_eq!(msg, format!("bad page {number}"));
                    }
                    Ok(_) => panic!("EXPECTED_BAD_PARAMS: {command} pages {pages} was accepted"),
                    Err(other) => panic!("EXPECTED_BAD_PARAMS: {command} pages {pages} returned {other:?}"),
                }
                assert_eq!(before, serde_json::to_value(&session.doc().unwrap().doc).unwrap());
            }
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn zero_and_maximum_export_pages_are_bad_params() {
    rejects(&[0, u64::MAX]);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn page_value_that_truncates_to_valid_page_is_bad_params() {
    rejects(&[(1u64 << 32) + 1]);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn page_value_that_truncates_to_zero_is_bad_params() {
    rejects(&[1u64 << 32]);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn page_one_exports_in_scalar_and_array_forms() {
    let mut session = Session::new();
    session.execute("file.new", &json!({"pages": 1})).unwrap();
    let before = serde_json::to_value(&session.doc().unwrap().doc).unwrap();
    for (command, count) in [("file.exportPdf", "pages"), ("file.exportPptx", "slides")] {
        for pages in [json!(1), json!([1])] {
            let result = session.execute(command, &json!({"pages": pages})).unwrap();
            assert_eq!(result[count], 1);
            assert!(!result["base64"].as_str().unwrap().is_empty());
            assert_eq!(before, serde_json::to_value(&session.doc().unwrap().doc).unwrap());
        }
    }
}
