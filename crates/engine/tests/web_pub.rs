// Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0
//! Headless wasm/Node smoke check of the browser's byte-based Publisher import path.
use designcraft_engine::{Session, cmd::base64_encode};
use serde_json::json;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn publisher_bytes_open_as_an_unsaved_document() {
    let bytes = include_bytes!("../../pub/tests/data/hello.pub");
    // The web UI strips the extension before dispatch; signature detection must work too.
    for name in ["hello.pub", "hello"] {
        let mut session = Session::new();
        let result = session.execute("file.openBytes", &json!({"name": name, "base64": base64_encode(bytes)})).unwrap();
        assert_eq!(result["index"], 0);
        assert!(result["warnings"].is_array());
        let state = session.doc().unwrap();
        assert_eq!(state.doc.title, "hello");
        assert!(state.path.is_none(), "opening Publisher bytes must not create a saved source path");
        assert_eq!(state.doc.page_count(), 1);
        assert_eq!(state.doc.spreads.iter().map(|spread| spread.items.len()).sum::<usize>(), 2);
        assert!(state.doc.stories.values().any(|story| story.text == "Hello from DesignCraft"));
        assert_eq!(session.documents().len(), 1);
    }
}
