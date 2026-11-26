#![cfg(target_arch = "wasm32")]

use std::sync::Arc;

use designcraft_engine::Session;
use serde_json::json;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn directory_paths_cannot_complete_a_browser_save() {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    session.execute("frame.create", &json!({"rect": [36, 36, 100, 100], "content": "unassigned"})).unwrap();
    assert!(session.doc().unwrap().is_dirty());
    for command in ["file.save", "file.saveAs", "file.saveACopy"] {
        for path in ["folder/", "folder/."] {
            let before = session.doc().unwrap();
            let (old_path, revision, saved_revision) = (before.path.clone(), before.revision, before.saved_revision);
            let (document, saved_document) = (before.doc.clone(), before.saved_doc.clone());
            let error = session.execute(command, &json!({"path": path})).unwrap_err();
            assert!(error.to_string().contains("path must name a file"), "{command} {path}: {error}");
            let after = session.doc().unwrap();
            assert_eq!((after.path.clone(), after.revision, after.saved_revision), (old_path, revision, saved_revision));
            assert!(Arc::ptr_eq(&after.doc, &document) && Arc::ptr_eq(&after.saved_doc, &saved_document));
            assert!(after.is_dirty());
        }
    }

    // The browser calls Save As with a normal name after its download succeeds.
    session.execute("file.saveAs", &json!({"path": "proof.designcraft"})).unwrap();
    let saved = session.doc().unwrap();
    assert_eq!(saved.path.as_deref(), Some("proof.designcraft"));
    assert_eq!(saved.saved_revision, saved.revision);
    assert!(!saved.is_dirty());
}
