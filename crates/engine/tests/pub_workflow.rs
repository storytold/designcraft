// Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0
//! The Publisher user's edit/save/print/export workflow through the same engine commands as UI.
#![cfg(not(target_arch = "wasm32"))]

use designcraft_doc::{Content, ItemId};
use designcraft_engine::Session;
use serde_json::json;

#[test]
fn publisher_flyer_remains_original_while_native_copy_is_edited_saved_printed_and_exported() {
    let dir = tempfile::tempdir().unwrap();
    let original = include_bytes!("../../pub/tests/data/hello.pub");
    let source = dir.path().join("flyer.pub");
    std::fs::write(&source, original).unwrap();
    let mut session = Session::new();
    session.execute("file.open", &json!({"path": source})).unwrap();
    let state = session.doc().unwrap();
    assert_eq!(state.doc.title, "flyer");
    assert_eq!(state.path, None);
    assert_eq!(state.save_suggestion(), dir.path().join("flyer.designcraft").to_string_lossy());
    assert_eq!(std::fs::read(&source).unwrap(), original);
    let text = state.doc.spreads[0].items.iter().find(|i| i.is_text_frame()).unwrap();
    let text_id = text.id;
    let Content::Text(frame) = &text.content else { panic!("not text") };
    let story = frame.story;
    let shape_id = state.doc.spreads[0].items.iter().find(|i| !i.is_text_frame()).unwrap().id;
    for id in [text_id, shape_id] {
        session.execute("selection.set", &json!({"ids": [id.0]})).unwrap();
        assert_eq!(session.doc().unwrap().selection.items, vec![id]);
        let old = session.doc().unwrap().doc.item(id).unwrap().bounds();
        session.execute("transform.move", &json!({"dx": 9, "dy": 12})).unwrap();
        let moved = session.doc().unwrap().doc.item(id).unwrap().bounds();
        assert!((moved.x0 - old.x0 - 9.0).abs() < 1e-6);
        assert!((moved.y0 - old.y0 - 12.0).abs() < 1e-6);
        session
            .execute(
                "transform.resize",
                &json!({"from": [moved.x0,moved.y0,moved.x1,moved.y1], "to": [moved.x0,moved.y0,moved.x1+18.0,moved.y1+24.0]}),
            )
            .unwrap();
        let resized = session.doc().unwrap().doc.item(id).unwrap().bounds();
        assert!((resized.width() - moved.width() - 18.0).abs() < 1e-6);
        assert!((resized.height() - moved.height() - 24.0).abs() < 1e-6);
    }
    session.execute("text.select", &json!({"story": story.0, "anchor": 0, "focus": 5})).unwrap();
    session.execute("text.insert", &json!({"text": "Edited"})).unwrap();
    assert!(session.doc().unwrap().doc.story(story).unwrap().text.starts_with("Edited"));
    let new_text = session.execute("frame.create", &json!({"rect": [72,400,300,440], "content": "text", "text": "Added text"})).unwrap();
    let new_shape = session.execute("frame.create", &json!({"rect": [320,400,360,440], "shape": "ellipse"})).unwrap();
    let png = designcraft_render::Rendered { width: 2, height: 2, pixels: vec![255; 16] }.to_png();
    session.execute("selection.set", &json!({"ids": []})).unwrap();
    session.execute("file.place", &json!({"base64": designcraft_engine::cmd::base64_encode(&png), "name": "added.png", "x":72,"y":480})).unwrap();
    assert_eq!(session.doc().unwrap().doc.spreads[0].items.len(), 5);
    assert!(session.doc().unwrap().doc.item(ItemId(new_shape["id"].as_u64().unwrap())).is_some());
    assert_eq!(session.doc().unwrap().doc.story(designcraft_doc::StoryId(new_text["story"].as_u64().unwrap())).unwrap().text, "Added text");
    assert!(session.doc().unwrap().is_dirty());
    assert!(session.execute("file.save", &json!({})).is_err());
    // Even an explicit source filename cannot overwrite Publisher: only native format is saved.
    let refused_extension = session.execute("file.save", &json!({"path": source})).unwrap();
    assert!(refused_extension["path"].as_str().unwrap().ends_with(".pub.designcraft"));
    assert_eq!(std::fs::read(&source).unwrap(), original);
    let destination = dir.path().join("flyer.designcraft");
    session.execute("file.saveAs", &json!({"path": destination})).unwrap();
    assert!(!session.doc().unwrap().is_dirty());
    let expected = serde_json::to_value(&session.doc().unwrap().doc).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.open", &json!({"path": destination})).unwrap();
    assert_eq!(serde_json::to_value(&reopened.doc().unwrap().doc).unwrap(), expected);
    let printed = reopened.execute("file.print", &json!({"systemDialog": true, "dryRun": true})).unwrap();
    assert_eq!(printed["pages"], 1);
    assert!(printed["bytes"].as_u64().unwrap() > 500);
    assert_eq!(designcraft_engine::find_command("file.print").unwrap().shortcut, Some("Cmd+P"));
    let pdf_path = dir.path().join("flyer.pdf");
    let exported = reopened.execute("file.exportPdf", &json!({"path": pdf_path})).unwrap();
    assert_eq!(exported["pages"], 1);
    let pdf = std::fs::read(pdf_path).unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
    assert_eq!(designcraft_render::pdf_page_count(&pdf), Some(1));
    assert_eq!(std::fs::read(&source).unwrap(), original);
}
