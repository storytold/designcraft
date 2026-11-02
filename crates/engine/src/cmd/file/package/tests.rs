//! Packages handed over as files: a folder's files (`file.openPackage`), a zip (`file.openBytes`)
//! and a `Document Fonts` folder for a document already open (`font.loadDocumentFonts`).

use std::io::Write;

use designcraft_fonts::testing::font_with;
use serde_json::{Value, json};

use super::{beside_document, folder_fonts, package_document};
use crate::Session;
use crate::cmd::file::bytes::base64_encode;

/// A `.designcraft` file whose text ("Hello") is set in `family`.
fn document_bytes(family: &str) -> Vec<u8> {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Hello"})).unwrap();
    s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 5})).unwrap();
    s.execute("type.char", &json!({"fontFamily": family, "fontStyle": "Regular"})).unwrap();
    let r = s.execute("file.serialize", &json!({})).unwrap();
    crate::cmd::file::bytes::base64_decode(r["base64"].as_str().unwrap())
}

fn font(family: &str) -> Vec<u8> {
    font_with(family, &['H', 'e', 'l', 'o']).unwrap()
}

/// The active document's font.list entry for `family`.
fn font_entry(s: &mut Session, family: &str) -> Value {
    let l = s.execute("font.list", &json!({})).unwrap();
    l.as_array().unwrap().iter().find(|f| f["family"] == family).cloned().unwrap_or_else(|| panic!("{family} in {l}"))
}

fn files(entries: &[(&str, &[u8])]) -> Value {
    Value::Array(entries.iter().map(|(p, b)| json!({"path": p, "base64": base64_encode(b)})).collect())
}

fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in entries {
        w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

#[test]
fn the_package_document_and_its_fonts_are_found_by_path() {
    let paths = [
        "Issue 12/Links/cover.png",
        "Issue 12/Old/Draft.designcraft",
        "Issue 12/Issue 12.idml",
        "Issue 12/._Issue 12.idml",
        "__MACOSX/Issue 12/Issue 12.idml",
    ];
    assert_eq!(package_document(paths.into_iter()), Some("Issue 12/Issue 12.idml"), "the one nearest the top, not a resource fork");
    assert_eq!(package_document(["a.png", "b.txt"].into_iter()), None);
    let doc = "Issue 12/Issue 12.idml";
    assert!(beside_document(doc, "Issue 12/Document Fonts/Palatino.ttf"));
    assert!(beside_document(doc, "Issue 12/document fonts/Palatino.OTF"), "the folder's case doesn't matter");
    assert!(!beside_document(doc, "Issue 12/Document Fonts/._Palatino.ttf"));
    assert!(!beside_document(doc, "Issue 12/Document Fonts/Extra/Palatino.ttf"), "not in subfolders, as on the desktop");
    assert!(!beside_document(doc, "Issue 12/Old/Document Fonts/Palatino.ttf"));
    assert!(!beside_document(doc, "Issue 12/Document Fonts/Palatino.woff2"));
    assert!(beside_document("Brochure.designcraft", "Document Fonts/a.ttf"), "a package without a top folder");
    // A folder handed over for a document already open: its Document Fonts, else all its fonts.
    assert_eq!(folder_fonts(["Issue/Issue.idml", "Issue/Document Fonts/a.ttf", "Issue/Links/b.ttf"].into_iter()), ["Issue/Document Fonts/a.ttf"]);
    assert_eq!(
        folder_fonts(["Document Fonts/a.ttf", "Document Fonts/b.otf", "Document Fonts/c.txt"].into_iter()),
        ["Document Fonts/a.ttf", "Document Fonts/b.otf"]
    );
}

/// A package folder handed over as files opens its document with the fonts beside it, for that
/// document alone; the document opens saved.
#[test]
fn a_package_folder_opens_with_its_document_fonts() {
    const FAMILY: &str = "Package Folder Test Family";
    let doc = document_bytes(FAMILY);
    let mut s = Session::new();
    let r = s
        .execute(
            "file.openPackage",
            &json!({"files": files(&[
                ("Issue/Issue.designcraft", &doc),
                ("Issue/Document Fonts/f.ttf", &font(FAMILY)),
                ("Issue/Document Fonts/broken.otf", b"not a font"),
                ("Issue/Links/cover.png", b"png"),
            ])}),
        )
        .unwrap();
    assert_eq!((r["document"].clone(), r["documentFonts"].clone()), (json!("Issue/Issue.designcraft"), json!(1)), "{r}");
    assert!(r["warnings"].to_string().contains("broken.otf"), "{r}");
    let f = font_entry(&mut s, FAMILY);
    assert_eq!((f["missing"].clone(), f["source"].clone()), (json!(false), json!("document")), "{f}");
    assert!(!s.doc().unwrap().is_dirty(), "opening isn't an edit");
    assert_eq!(s.doc().unwrap().title(), "Issue");
    // Another document doesn't see them.
    s.execute("file.openBytes", &json!({"name": "Other.designcraft", "base64": base64_encode(&doc)})).unwrap();
    assert_eq!(font_entry(&mut s, FAMILY)["missing"], true);
    assert!(s.execute("file.openPackage", &json!({"files": files(&[("a.png", b"x")])})).is_err(), "no document");
}

/// A zip of a package opens through file.openBytes like the folder; only the entries needed are
/// read, and a Mac's zip junk is left alone.
#[test]
fn a_zipped_package_opens_with_its_document_fonts() {
    const FAMILY: &str = "Package Zip Test Family";
    let doc = document_bytes(FAMILY);
    let bytes = zip(&[
        ("Issue/Issue.designcraft", &doc),
        ("Issue/Document Fonts/f.ttf", &font(FAMILY)),
        ("__MACOSX/Issue/Document Fonts/._f.ttf", b"junk"),
        ("Issue/Links/cover.png", b"png"),
    ]);
    assert!(super::is_zipped_package(&bytes));
    assert!(!super::is_zipped_package(&doc), "a DesignCraft file is a zip but not a package");
    let mut s = Session::new();
    let r = s.execute("file.openBytes", &json!({"name": "Issue.zip", "base64": base64_encode(&bytes)})).unwrap();
    assert_eq!(r["documentFonts"], 1, "{r}");
    assert_eq!(font_entry(&mut s, FAMILY)["missing"], false);
    assert!(!s.doc().unwrap().is_dirty());
    // A zip without a document isn't a package: an error, as before.
    let other = zip(&[("notes.txt", b"hello")]);
    assert!(!super::is_zipped_package(&other));
    assert!(s.execute("file.openBytes", &json!({"name": "notes.zip", "base64": base64_encode(&other)})).is_err());
}

/// A document opened by itself gets its Document Fonts afterwards from a folder handed over:
/// the package folder or the Document Fonts folder itself. Not an edit.
#[test]
fn document_fonts_load_for_a_document_already_open() {
    const FAMILY: &str = "Package Later Test Family";
    let doc = document_bytes(FAMILY);
    let mut s = Session::new();
    s.execute("file.openBytes", &json!({"name": "Issue.designcraft", "base64": base64_encode(&doc)})).unwrap();
    assert_eq!(font_entry(&mut s, FAMILY)["missing"], true);
    let revision = s.doc().unwrap().revision;
    let none = s.execute("font.loadDocumentFonts", &json!({"files": files(&[("Issue/Issue.designcraft", &doc)])}));
    assert!(none.unwrap_err().to_string().contains("no font files"));
    let r = s.execute("font.loadDocumentFonts", &json!({"files": files(&[("Document Fonts/f.ttf", &font(FAMILY))])})).unwrap();
    assert_eq!(r["documentFonts"], 1, "{r}");
    let st = s.doc().unwrap();
    assert!(!st.is_dirty(), "not an edit");
    assert_ne!(st.revision, revision, "drawn again with the fonts");
    assert_eq!(font_entry(&mut s, FAMILY)["missing"], false);
}
