use std::sync::Arc;

use designcraft_doc::ItemId;
use designcraft_engine::{Session, cmd::base64_encode};
use serde_json::{Value, json};

fn document_with_text() -> (Session, Value) {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let frame = session
        .execute("frame.create", &json!({"rect": [0, 0, 100, 100], "content": "text", "text": "Keep this artwork", "caret": false}))
        .unwrap()["id"]
        .clone();
    (session, frame)
}

fn nest_groups(session: &mut Session, mut group: Value) {
    // Fixed synthetic reproduction, not a stress sweep: this legally edited document exceeded
    // the reader's JSON nesting budget. Exercise commands (and native preview generation on
    // Save), even if that makes this regression slower than a metadata-only format test.
    for _ in 0..48 {
        let sibling = session.execute("frame.create", &json!({"rect": [120, 0, 130, 10]})).unwrap()["id"].clone();
        group = session.execute("object.group", &json!({"ids": [group, sibling]})).unwrap()["id"].clone();
    }
}

/// The readability check encodes the document without image payloads; reopening must still
/// read the real archive's embedded image bytes.
#[test]
fn a_document_with_a_placed_image_saves_and_reopens_with_its_image_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("placed.designcraft");
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let png = designcraft_render::Rendered { width: 4, height: 3, pixels: vec![200; 4 * 3 * 4] }.to_png();
    let placed = session.execute("file.place", &json!({"base64": base64_encode(&png), "name": "photo.png", "x": 0, "y": 0, "width": 72})).unwrap();
    let id = ItemId(placed["id"].as_u64().unwrap());

    session.execute("file.saveAs", &json!({"path": path})).expect("a document with a placed image saves");
    assert!(!session.doc().unwrap().is_dirty());

    let mut reopened = Session::new();
    reopened.execute("file.open", &json!({"path": path})).expect("the saved document with an image reopens");
    let doc = &reopened.doc().unwrap().doc;
    let asset = doc.item(id).and_then(|item| item.graphic()).map(|graphic| graphic.asset).expect("the placed graphic survives");
    assert_eq!(doc.assets[&asset].data.load().unwrap().as_slice(), png.as_slice(), "the embedded image bytes survive");
}

#[test]
fn nested_group_save_is_readable_or_preserves_the_good_file_and_dirty_edits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested.designcraft");
    let (mut session, root) = document_with_text();
    session.execute("file.saveAs", &json!({"path": path})).unwrap();
    let good_bytes = std::fs::read(&path).unwrap();
    let mut baseline = Session::new();
    baseline.execute("file.open", &json!({"path": path})).expect("the previous saved file must be readable");
    nest_groups(&mut session, root);
    let before = session.doc().unwrap().clone();
    assert!(before.is_dirty());

    let saved = session.execute("file.save", &json!({}));
    let mut reopened = Session::new();
    match saved {
        Ok(_) => {
            assert!(!session.doc().unwrap().is_dirty(), "acknowledged Save is clean");
            reopened.execute("file.open", &json!({"path": path})).expect("acknowledged Save must produce a readable replacement");
            assert_eq!(*reopened.doc().unwrap().doc, *session.doc().unwrap().doc, "the complete artwork and text survive Save and reopen");
        }
        Err(_) => {
            let after = session.doc().unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), good_bytes, "refused Save leaves previous good bytes alone");
            assert!(after.is_dirty(), "refused Save retains dirty edits");
            assert_eq!(after.path, before.path, "refused Save keeps the original path");
            assert!(Arc::ptr_eq(&after.doc, &before.doc), "refused Save keeps the edited document");
            assert!(Arc::ptr_eq(&after.saved_doc, &before.saved_doc), "refused Save does not acknowledge edits");
            assert_eq!((after.revision, after.saved_revision), (before.revision, before.saved_revision));
            reopened.execute("file.open", &json!({"path": path})).expect("refused Save must leave a readable baseline");
            assert_eq!(*reopened.doc().unwrap().doc, *baseline.doc().unwrap().doc);
        }
    }
}

#[test]
fn nested_group_recovery_is_readable_or_preserves_the_previous_recoverable_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let recovery = dir.path().join("recovery");
    let (mut session, root) = document_with_text();
    session.execute("file.recovery.save", &json!({"dir": recovery})).unwrap();
    let uid = session.doc().unwrap().uid;
    let checkpoint = recovery.join(format!("{uid}.designcraft"));
    let metadata = recovery.join(format!("{uid}.json"));
    let good_bytes = std::fs::read(&checkpoint).unwrap();
    let good_metadata = std::fs::read(&metadata).unwrap();
    let baseline = designcraft_format::load(&good_bytes).expect("the previous checkpoint must be readable");
    nest_groups(&mut session, root);
    let before = session.doc().unwrap().clone();

    let saved = session.execute("file.recovery.save", &json!({"dir": recovery}));
    assert!(session.doc().unwrap().is_dirty(), "recovery must not acknowledge normal Save");
    assert!(Arc::ptr_eq(&session.doc().unwrap().doc, &before.doc), "checkpointing must not change dirty edits");
    let expected = match saved {
        Ok(_) => {
            let decoded = designcraft_format::load(&std::fs::read(&checkpoint).unwrap()).expect("accepted checkpoint must be readable");
            assert_eq!(decoded, *before.doc, "accepted recovery preserves the complete edited document");
            decoded
        }
        Err(_) => {
            assert_eq!(std::fs::read(&checkpoint).unwrap(), good_bytes, "refused checkpoint replacement preserves good recovery bytes");
            assert_eq!(std::fs::read(&metadata).unwrap(), good_metadata, "refusal does not consume or replace checkpoint metadata");
            baseline
        }
    };
    let mut recovered = Session::new();
    let result = recovered.execute("file.recovery.open", &json!({"dir": recovery})).unwrap();
    assert!(!result["opened"].as_array().unwrap().is_empty(), "the surviving checkpoint must actually recover");
    assert_eq!(*recovered.doc().unwrap().doc, expected, "the recoverable document retains artwork and text");
}
