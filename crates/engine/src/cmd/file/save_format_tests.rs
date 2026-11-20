//! Preferences › File Handling › Save documents as (`saveFormat`): a `.idml` name is saved as
//! IDML, and with IDML chosen an opened `.idml` file is saved back to itself.

use std::path::PathBuf;

use serde_json::json;

use crate::Session;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dc-save-format-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A new document holding a text frame with `text`.
fn document_with(s: &mut Session, text: &str) {
    s.execute("file.new", &json!({})).unwrap();
    s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": text})).unwrap();
}

/// The text of the active document's stories, joined.
fn texts(s: &Session) -> String {
    let d = &s.doc().unwrap().doc;
    d.stories.values().map(|st| st.text.as_str()).collect::<Vec<_>>().join("|")
}

#[test]
fn a_save_to_an_idml_name_writes_idml() {
    let dir = temp_dir("saveas");
    let mut s = Session::new();
    document_with(&mut s, "Hello IDML");
    let path = dir.join("Brochure.idml");
    s.execute("file.saveAs", &json!({"path": path.to_str().unwrap()})).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(designcraft_idml::is_idml(&bytes), "an IDML package, not DesignCraft's format");
    assert!(!s.doc().unwrap().is_dirty(), "the document is saved");
    // Save again: the same file, as IDML.
    s.execute("file.save", &json!({})).unwrap();
    assert!(designcraft_idml::is_idml(&std::fs::read(&path).unwrap()));
    // It opens again with its text.
    let mut t = Session::new();
    t.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    assert!(texts(&t).contains("Hello IDML"), "{}", texts(&t));
    // Any other name is DesignCraft's own format, as before.
    let native = dir.join("Brochure.designcraft");
    s.execute("file.saveAs", &json!({"path": native.to_str().unwrap()})).unwrap();
    assert!(!designcraft_idml::is_idml(&std::fs::read(&native).unwrap()));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn save_a_copy_to_an_idml_name_writes_idml() {
    let dir = temp_dir("copy");
    let mut s = Session::new();
    document_with(&mut s, "Copied");
    let path = dir.join("Copy.idml");
    s.execute("file.saveACopy", &json!({"path": path.to_str().unwrap()})).unwrap();
    assert!(designcraft_idml::is_idml(&std::fs::read(&path).unwrap()));
    assert!(s.doc().unwrap().path.is_none(), "a copy doesn't change which file the document is");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn with_idml_chosen_an_opened_idml_file_saves_back_to_itself() {
    let dir = temp_dir("roundtrip");
    let path = dir.join("Magazine.idml");
    let p = path.to_str().unwrap();
    let mut s = Session::new();
    document_with(&mut s, "First draft");
    s.execute("file.saveAs", &json!({"path": p})).unwrap();

    // DesignCraft's own format chosen (the default): the opened .idml starts unsaved, so Save
    // never overwrites it unasked.
    let mut a = Session::new();
    a.execute("file.open", &json!({"path": p})).unwrap();
    assert!(a.doc().unwrap().path.is_none());
    assert!(a.execute("file.save", &json!({})).is_err(), "Save asks for a name");

    // IDML chosen: the document is that file, unchanged until edited, and Save writes IDML to it.
    let mut b = Session::new();
    let r = b.execute("prefs.set", &json!({"saveFormat": "idml"})).unwrap();
    assert_eq!(r["saveFormat"], "idml");
    b.execute("file.open", &json!({"path": p})).unwrap();
    assert_eq!(b.doc().unwrap().path.as_deref(), Some(p));
    assert!(!b.doc().unwrap().is_dirty());
    b.execute("frame.create", &json!({"rect": [72, 300, 400, 400], "content": "text", "text": "Second draft"})).unwrap();
    b.execute("file.save", &json!({})).unwrap();
    assert!(designcraft_idml::is_idml(&std::fs::read(&path).unwrap()));
    let mut c = Session::new();
    c.execute("file.open", &json!({"path": p})).unwrap();
    let t = texts(&c);
    assert!(t.contains("First draft") && t.contains("Second draft"), "{t}");
    // An IDML package not named .idml opens (by its content) but stays unsaved: Save never
    // writes to a file whose name doesn't say IDML.
    let unnamed = dir.join("Magazine copy");
    std::fs::copy(&path, &unnamed).unwrap();
    b.execute("file.open", &json!({"path": unnamed.to_str().unwrap()})).unwrap();
    assert_eq!(b.doc().unwrap().path, None);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_save_format_preference_takes_only_known_formats() {
    let mut s = Session::new();
    assert_eq!(s.prefs.save_format, crate::SaveFormat::Designcraft, "DesignCraft's own format by default");
    assert!(s.execute("prefs.set", &json!({"saveFormat": "pdf"})).is_err());
    assert_eq!(s.prefs.save_format, crate::SaveFormat::Designcraft, "a refused value changes nothing");
    // Preferences saved before the setting existed load with the default.
    let old: crate::Prefs = serde_json::from_str(r#"{"linkTextFiles": true}"#).unwrap();
    assert_eq!(old.save_format, crate::SaveFormat::Designcraft);
    assert_eq!(crate::SaveFormat::of_path("/a/B.IDML"), crate::SaveFormat::Idml);
    assert_eq!(crate::SaveFormat::of_path("/a/B.designcraft"), crate::SaveFormat::Designcraft);
    assert_eq!(crate::SaveFormat::of_path("/a/idml"), crate::SaveFormat::Designcraft, "a name, not an extension");
}

/// CodeRabbit on #1105: a save as IDML would drop editorial notes (and turn endnotes into plain
/// text), then mark the document saved and drop its recovery copy, so the loss would be silent
/// and for good. Such a save is refused unless `allowLoss`; file.idmlLosses says what it loses.
#[test]
fn an_idml_save_that_would_lose_content_is_refused_unless_allowed() {
    let dir = temp_dir("lossy");
    let mut s = Session::new();
    document_with(&mut s, "Has a note");
    assert_eq!(s.execute("file.idmlLosses", &json!({})).unwrap(), json!({"editorialNotes": 0, "endnotes": 0}));
    s.execute("note.new", &json!({"text": "Check this"})).unwrap();
    assert_eq!(s.execute("file.idmlLosses", &json!({})).unwrap(), json!({"editorialNotes": 1, "endnotes": 0}));

    let path = dir.join("Notes.idml");
    let copy = dir.join("Copy.idml");
    for (cmd, p) in [("file.saveAs", &path), ("file.saveACopy", &copy)] {
        let e = s.execute(cmd, &json!({"path": p.to_string_lossy()})).unwrap_err();
        assert!(e.to_string().starts_with(super::bytes::LOSSY_IDML_SAVE), "{cmd}: {e}");
        assert!(!p.exists(), "{cmd}: nothing written");
    }
    assert!(s.doc().unwrap().is_dirty() && s.doc().unwrap().path.is_none(), "the document is as it was");

    // Saved as DesignCraft, nothing is lost and nothing is asked.
    s.execute("file.saveAs", &json!({"path": dir.join("Notes.designcraft").to_string_lossy()})).unwrap();
    // Allowed, the IDML is written; the document's own file is then IDML, and ⌘S is refused again
    // while there is something to lose.
    s.execute("file.saveAs", &json!({"path": path.to_string_lossy(), "allowLoss": true})).unwrap();
    assert!(designcraft_idml::is_idml(&std::fs::read(&path).unwrap()));
    s.execute("endnote.insert", &json!({"text": "An endnote."})).unwrap();
    assert_eq!(s.execute("file.idmlLosses", &json!({})).unwrap(), json!({"editorialNotes": 1, "endnotes": 1}));
    assert!(s.execute("file.save", &json!({})).is_err());
    assert!(s.doc().unwrap().is_dirty());
    s.execute("file.save", &json!({"allowLoss": true})).unwrap();
    assert!(!s.doc().unwrap().is_dirty());
    s.execute("file.saveACopy", &json!({"path": copy.to_string_lossy(), "allowLoss": true})).unwrap();
    assert!(copy.exists());
    let _ = std::fs::remove_dir_all(&dir);
}
