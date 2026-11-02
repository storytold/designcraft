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

/// Answers every file dialog with `path` (as typed in it).
fn answer_with(app: &mut DesignApp, path: &std::path::Path) {
    let path = path.to_path_buf();
    app.answer_dialogs(move |request| match request {
        craft_dialogs::Request::File(_) => craft_dialogs::Answer::Paths(vec![path.clone()]),
        _ => craft_dialogs::Answer::Cancelled,
    });
}

/// A new document with a text frame holding an editorial note: IDML can't keep it.
fn document_with_a_note(app: &mut DesignApp, title: &str) {
    app.run("file.new", json!({"title": title})).unwrap();
    app.run("frame.create", json!({"rect": [72, 72, 400, 200], "content": "text", "text": "Hello"})).unwrap();
    app.run("note.new", json!({"text": "Check this"})).unwrap();
}

/// CodeRabbit on #1105: a save as IDML that would lose editorial notes asks first. Cancel saves
/// nothing; Save Anyway writes the IDML; Save as DesignCraft… asks where and writes DesignCraft's
/// format instead, which keeps the notes.
#[test]
fn an_idml_save_that_would_lose_notes_asks_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("prefs.set", json!({"saveFormat": "idml"})).unwrap();
    document_with_a_note(&mut app, "Notes");
    let idml = dir.path().join("Notes.idml");
    answer_with(&mut app, &idml);

    // Cancel: nothing written, still unsaved.
    app.run("app.save", json!({})).unwrap();
    app.settle_dialogs();
    let d = app.ui.dialog.as_ref().expect("the IDML loss question");
    assert_eq!((d.id.as_str(), &d.fields["editorialNotes"], &d.fields["endnotes"]), ("idmlLoss", &json!(1), &json!(0)));
    crate::dialogs::cancel(&mut app);
    assert!(app.ui.dialog.is_none() && app.lossy_save.is_none());
    assert!(!idml.exists());
    assert!(app.session.active().unwrap().is_dirty());

    // Save Anyway: the IDML is written and the document counts as saved.
    app.run("app.save", json!({})).unwrap();
    app.settle_dialogs();
    crate::dialogs::confirm(&mut app).unwrap();
    assert!(designcraft_idml::is_idml(&std::fs::read(&idml).unwrap()));
    assert!(!app.session.active().unwrap().is_dirty());
    assert_eq!(app.session.active().unwrap().path.as_deref(), Some(idml.to_str().unwrap()));

    // ⌘S to its own IDML file asks again; Save as DesignCraft… saves as DesignCraft instead.
    app.run("note.new", json!({"text": "And this"})).unwrap();
    let before = std::fs::read(&idml).unwrap();
    let native = dir.path().join("Notes.designcraft");
    answer_with(&mut app, &native);
    app.run("app.save", json!({})).unwrap();
    assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("idmlLoss"));
    app.ui.dialog.as_mut().unwrap().fields.insert("asDesigncraft".into(), json!(true));
    crate::dialogs::confirm(&mut app).unwrap();
    app.settle_dialogs();
    assert_eq!(std::fs::read(&idml).unwrap(), before, "the IDML file is untouched");
    let saved = std::fs::read(&native).unwrap();
    assert!(!designcraft_idml::is_idml(&saved));
    assert_eq!(app.session.active().unwrap().path.as_deref(), Some(native.to_str().unwrap()));
    assert!(!app.session.active().unwrap().is_dirty());

    // Save a Copy to an IDML name asks too; Cancel writes nothing.
    let copy = dir.path().join("Copy.idml");
    app.run("app.saveCopyDialog", json!({"path": copy.to_string_lossy()})).unwrap();
    assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("idmlLoss"));
    crate::dialogs::cancel(&mut app);
    assert!(!copy.exists());
}

/// In a browser, the download asks the same question; Save as DesignCraft… downloads DesignCraft's
/// format.
#[test]
fn a_download_that_would_lose_notes_asks_first() {
    let dir = tempfile::tempdir().unwrap();
    let title = dir.path().join("Notes").to_string_lossy().to_string();
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let got: Arc<Mutex<Vec<(String, Vec<u8>)>>> = Arc::default();
    let sink = got.clone();
    app.services.download = Some(Box::new(move |name: &str, bytes: &[u8]| {
        sink.lock().unwrap().push((name.to_string(), bytes.to_vec()));
        Ok(())
    }));
    app.run("prefs.set", json!({"saveFormat": "idml"})).unwrap();
    document_with_a_note(&mut app, &title);
    app.run("app.save", json!({})).unwrap();
    assert!(got.lock().unwrap().is_empty(), "nothing downloaded before the answer");
    app.ui.dialog.as_mut().unwrap().fields.insert("asDesigncraft".into(), json!(true));
    crate::dialogs::confirm(&mut app).unwrap();
    let got = got.lock().unwrap();
    let [(name, bytes)] = got.as_slice() else { panic!("one download: {:?}", got.len()) };
    assert!(name.ends_with("Notes.designcraft"), "{name}");
    assert!(!designcraft_idml::is_idml(bytes));
    assert!(!app.session.active().unwrap().is_dirty());
}

/// Don't ask again (with Save Anyway) stops the question for that document, for this session
/// only (it is kept by the app, not in the document or the preferences): another document is
/// still asked about.
#[test]
fn dont_ask_again_holds_for_the_document_in_this_session() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    document_with_a_note(&mut app, "Notes");
    let idml = dir.path().join("Notes.idml");
    // Not ticked: asked again next time.
    app.run("app.saveDialog", json!({"path": idml.to_string_lossy()})).unwrap();
    assert_eq!(app.ui.dialog.as_ref().unwrap().fields["dontAskAgain"], false);
    crate::dialogs::confirm(&mut app).unwrap();
    app.run("note.new", json!({"text": "Two"})).unwrap();
    app.run("app.save", json!({})).unwrap();
    // Ticked, but Save as DesignCraft… chosen: it doesn't count.
    let d = app.ui.dialog.as_mut().expect("asked again");
    d.fields.insert("dontAskAgain".into(), json!(true));
    d.fields.insert("asDesigncraft".into(), json!(true));
    answer_with(&mut app, &dir.path().join("Notes.designcraft"));
    crate::dialogs::confirm(&mut app).unwrap();
    app.settle_dialogs();
    app.run("app.saveDialog", json!({"path": idml.to_string_lossy()})).unwrap();
    // Ticked with Save Anyway: saved, and not asked again for this document.
    app.ui.dialog.as_mut().expect("still asked").fields.insert("dontAskAgain".into(), json!(true));
    crate::dialogs::confirm(&mut app).unwrap();
    app.run("note.new", json!({"text": "Three"})).unwrap();
    app.run("app.save", json!({})).unwrap();
    assert!(app.ui.dialog.is_none(), "not asked again");
    assert!(!app.session.active().unwrap().is_dirty(), "saved as IDML");
    assert!(designcraft_idml::is_idml(&std::fs::read(&idml).unwrap()));
    // Another document is still asked about.
    document_with_a_note(&mut app, "Other");
    app.run("app.saveDialog", json!({"path": dir.path().join("Other.idml").to_string_lossy()})).unwrap();
    assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("idmlLoss"));
    crate::dialogs::cancel(&mut app);
}
