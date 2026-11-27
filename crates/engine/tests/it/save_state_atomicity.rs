use std::path::Path;
use std::sync::Arc;

use designcraft_engine::Session;
use serde_json::json;

fn linked_document(root: &Path) -> (Session, std::path::PathBuf) {
    let source = root.join("source/contacts.csv");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, "Name\nAda\n").unwrap();
    let original = root.join("original/template.designcraft");
    std::fs::create_dir_all(original.parent().unwrap()).unwrap();
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    session.execute("data.source.select", &json!({"path": source})).unwrap();
    session.execute("file.saveAs", &json!({"path": original})).unwrap();
    (session, original)
}

#[test]
fn failed_save_as_preserves_document_saved_state_history_and_original_file() {
    let temp = tempfile::tempdir().unwrap();
    let (mut session, original) = linked_document(temp.path());
    session.execute("frame.create", &json!({"rect": [0, 0, 50, 50]})).unwrap();
    session.execute("edit.undo", &json!({})).unwrap();
    let before = session.doc().unwrap().clone();
    let journal = session.journal.clone();
    let original_bytes = std::fs::read(&original).unwrap();
    let destination = temp.path().join("missing/elsewhere/template.designcraft");

    assert!(session.execute("file.saveAs", &json!({"path": destination})).is_err());

    let after = session.doc().unwrap();
    assert!(Arc::ptr_eq(&before.doc, &after.doc), "failed Save As must not change data-source paths");
    assert!(Arc::ptr_eq(&before.saved_doc, &after.saved_doc));
    assert_eq!(before.path, after.path);
    assert_eq!((before.revision, before.saved_revision, before.is_dirty()), (after.revision, after.saved_revision, after.is_dirty()));
    assert_eq!(before.doc.data_merge, after.doc.data_merge);
    assert_eq!(before.history.undo.len(), after.history.undo.len());
    assert_eq!(before.history.redo.len(), after.history.redo.len());
    assert_eq!(serde_json::to_value(&before.selection).unwrap(), serde_json::to_value(&after.selection).unwrap());
    assert_eq!(journal, session.journal);
    assert_eq!(original_bytes, std::fs::read(original).unwrap());
    assert!(!destination.exists());
}

#[test]
fn save_a_copy_rebases_its_data_source_without_changing_the_open_document() {
    let temp = tempfile::tempdir().unwrap();
    let (mut session, original) = linked_document(temp.path());
    let before = session.doc().unwrap().clone();
    let original_bytes = std::fs::read(&original).unwrap();
    let destination = temp.path().join("copies/nested/template.designcraft");
    std::fs::create_dir_all(destination.parent().unwrap()).unwrap();

    session.execute("file.saveACopy", &json!({"path": destination})).unwrap();

    let copy = designcraft_format::load(&std::fs::read(destination).unwrap()).unwrap();
    assert_eq!(copy.data_merge.sources[0].relative_path.as_deref().map(Path::new), Some(Path::new("../../source/contacts.csv")));
    let after = session.doc().unwrap();
    assert!(Arc::ptr_eq(&before.doc, &after.doc));
    assert!(Arc::ptr_eq(&before.saved_doc, &after.saved_doc));
    assert_eq!(before.path, after.path);
    assert_eq!((before.revision, before.saved_revision, before.is_dirty()), (after.revision, after.saved_revision, after.is_dirty()));
    assert_eq!(original_bytes, std::fs::read(original).unwrap());
}
