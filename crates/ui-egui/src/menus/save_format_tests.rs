//! Preferences › File Handling › Save documents as.

use std::sync::{Arc, Mutex};

use serde_json::json;

use crate::DesignApp;

#[test]
fn choosing_idml_makes_save_suggest_and_write_idml() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"title": "Brochure"})).unwrap();
    app.run("frame.create", json!({"rect": [72, 72, 400, 200], "content": "text", "text": "Hello"})).unwrap();
    // Preferences › File Handling: InDesign Markup.
    app.run("app.preferences", json!({})).unwrap();
    let d = app.ui.dialog.as_mut().unwrap();
    assert_eq!(d.fields["saveFormat"], "designcraft", "DesignCraft's own format by default");
    d.fields.insert("saveFormat".into(), json!("idml"));
    crate::dialogs::confirm(&mut app).unwrap();
    assert_eq!(app.session.prefs.save_format, designcraft_engine::SaveFormat::Idml);
    // Save: the dialog suggests a .idml name; a name typed without an extension gets .idml.
    let dir = tempfile::tempdir().unwrap();
    let typed = dir.path().join("Brochure").to_string_lossy().to_string();
    let suggested = Arc::new(Mutex::new(String::new()));
    let seen = suggested.clone();
    app.answer_dialogs(move |request| {
        let craft_dialogs::Request::File(dialog) = request else { return craft_dialogs::Answer::Cancelled };
        *seen.lock().unwrap() = dialog.file_name.clone().unwrap_or_default();
        craft_dialogs::Answer::Paths(vec![typed.clone().into()])
    });
    app.run("app.save", json!({})).unwrap();
    app.settle_dialogs();
    assert_eq!(*suggested.lock().unwrap(), "Brochure.idml");
    let path = dir.path().join("Brochure.idml");
    assert!(designcraft_idml::is_idml(&std::fs::read(&path).unwrap()));
    assert_eq!(app.session.active().unwrap().path.as_deref(), Some(path.to_str().unwrap()));
    // Back to DesignCraft's format: Save As suggests .designcraft again.
    app.run("app.preferences", json!({})).unwrap();
    let d = app.ui.dialog.as_mut().unwrap();
    assert_eq!(d.fields["saveFormat"], "idml", "the dialog shows the current choice");
    d.fields.insert("saveFormat".into(), json!("designcraft"));
    crate::dialogs::confirm(&mut app).unwrap();
    let seen = suggested.clone();
    app.answer_dialogs(move |request| {
        let craft_dialogs::Request::File(dialog) = request else { return craft_dialogs::Answer::Cancelled };
        *seen.lock().unwrap() = dialog.file_name.clone().unwrap_or_default();
        craft_dialogs::Answer::Cancelled
    });
    app.run("app.saveDialog", json!({})).unwrap();
    app.settle_dialogs();
    // Started beside the saved file.
    let suggested = suggested.lock().unwrap().clone();
    assert_eq!(std::path::Path::new(&suggested).file_name().unwrap(), "Brochure.designcraft", "{suggested}");
}

#[test]
fn a_typed_extension_is_kept() {
    assert_eq!(super::run::with_save_extension("/a/Brochure", "idml"), "/a/Brochure.idml");
    assert_eq!(super::run::with_save_extension("/a/Brochure.idml", "designcraft"), "/a/Brochure.idml");
    assert_eq!(super::run::with_save_extension("/a/Brochure.designcraft", "idml"), "/a/Brochure.designcraft");
    assert_eq!(super::run::with_save_extension("/a/Brochure.IDML", "designcraft"), "/a/Brochure.IDML");
    // A dot in the name isn't an extension: the chosen format is added.
    assert_eq!(super::run::with_save_extension("/a/Issue 3.1", "idml"), "/a/Issue 3.1.idml");
    assert_eq!(super::run::with_save_extension("/a/v1.2 Brochure", "designcraft"), "/a/v1.2 Brochure.designcraft");
}
/// The web build saves by download: the download is named and written in the chosen format.
#[test]
fn a_download_save_follows_the_save_format() {
    for (format, ext) in [("idml", "idml"), ("designcraft", "designcraft")] {
        let dir = tempfile::tempdir().unwrap();
        // The title is a path in the temporary folder: on the desktop, marking the download
        // saved writes there too.
        let title = dir.path().join("Brochure").to_string_lossy().to_string();
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let got: Arc<Mutex<Vec<(String, Vec<u8>)>>> = Arc::default();
        let sink = got.clone();
        app.services.download = Some(Box::new(move |name: &str, bytes: &[u8]| {
            sink.lock().unwrap().push((name.to_string(), bytes.to_vec()));
            Ok(())
        }));
        app.run("prefs.set", json!({"saveFormat": format})).unwrap();
        app.run("file.new", json!({"title": title})).unwrap();
        app.run("frame.create", json!({"rect": [72, 72, 400, 200], "content": "text", "text": "Hello"})).unwrap();
        app.run("app.save", json!({})).unwrap();
        let got = got.lock().unwrap();
        let [(name, bytes)] = got.as_slice() else { panic!("one download: {:?}", got.len()) };
        assert!(name.ends_with(&format!("Brochure.{ext}")), "{name}");
        assert_eq!(designcraft_idml::is_idml(bytes), format == "idml", "{format}");
        assert!(!app.session.active().unwrap().is_dirty(), "the document counts as saved");
    }
}
