#![cfg(not(target_arch = "wasm32"))]

use designcraft_engine::Session;
use serde_json::json;
use std::path::Path;
fn dir(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new().prefix(&format!("dc-book-safety-{name}-")).tempdir().unwrap()
}
fn document(start: u32, custom: bool) -> designcraft_doc::Document {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages":2})).unwrap();
    let mut d = (*s.doc().unwrap().doc).clone();
    d.sections.first_mut().unwrap().start_number = Some(start);
    if custom {
        let mut style = d.styles.paragraph.get(1).unwrap().clone();
        style.name = "Synthetic Book Style".into();
        d.styles_mut().paragraph.push(style);
    }
    d
}
fn native(p: &Path, start: u32, custom: bool) {
    std::fs::write(p, designcraft_format::save(&document(start, custom)).unwrap()).unwrap();
}
fn idml(p: &Path, start: u32) {
    std::fs::write(p, designcraft_idml::export_idml(&document(start, false))).unwrap();
    assert!(designcraft_idml::import_idml(&std::fs::read(p).unwrap()).is_ok());
}
fn book(d: &Path, chapters: &[&Path]) -> Session {
    let mut s = Session::new();
    s.execute("book.new", &json!({"path":d.join("book.dcbook").to_string_lossy()})).unwrap();
    for p in chapters {
        s.execute("book.add", &json!({"path":p.to_string_lossy()})).unwrap();
    }
    s
}
#[test]
fn late_missing_chapter_does_not_partially_paginate_earlier_chapters() {
    let d = dir("late-missing");
    let first = d.path().join("first.designcraft");
    let later = d.path().join("later.designcraft");
    native(&first, 9, false);
    native(&later, 1, false);
    let mut s = book(d.path(), &[&first, &later]);
    let before = std::fs::read(&first).unwrap();
    std::fs::remove_file(later).unwrap();
    let result = s.execute("book.paginate", &json!({}));
    assert!(result.is_err());
    let after = std::fs::read(&first).unwrap();
    assert!(
        before == after,
        "book.paginate changed the first chapter before error: {result:?}; new first number {:?}",
        designcraft_format::load(&after).unwrap().sections.first().unwrap().start_number
    );
}
#[test]
fn late_corrupt_chapter_does_not_partially_sync_earlier_chapters() {
    let d = dir("late-corrupt");
    let source = d.path().join("source.designcraft");
    let first = d.path().join("first.designcraft");
    let later = d.path().join("later.designcraft");
    native(&source, 1, true);
    native(&first, 1, false);
    native(&later, 1, false);
    let mut s = book(d.path(), &[&source, &first, &later]);
    let before = std::fs::read(&first).unwrap();
    std::fs::write(later, b"not a document").unwrap();
    let result = s.execute("book.syncStyles", &json!({}));
    assert!(result.is_err());
    let after = std::fs::read(&first).unwrap();
    assert!(
        before == after,
        "book.syncStyles changed the first target before error: {result:?}; copied custom style {}",
        designcraft_format::load(&after).unwrap().styles.paragraph.iter().any(|s| s.name == "Synthetic Book Style")
    );
}
#[test]
fn paginate_does_not_replace_imported_idml_with_native_format() {
    let d = dir("idml-paginate");
    let chapter = d.path().join("chapter.idml");
    idml(&chapter, 9);
    let mut s = book(d.path(), &[&chapter]);
    let before = std::fs::read(&chapter).unwrap();
    let result = s.execute("book.paginate", &json!({}));
    let after = std::fs::read(&chapter).unwrap();
    assert!(
        before == after,
        "book.paginate rewrote imported IDML: {result:?}; IDML readable after {}, native readable after {}",
        designcraft_idml::import_idml(&after).is_ok(),
        designcraft_format::load(&after).is_ok()
    );
}
#[test]
fn style_sync_does_not_replace_imported_idml_with_native_format() {
    let d = dir("idml-sync");
    let source = d.path().join("source.designcraft");
    let chapter = d.path().join("chapter.idml");
    native(&source, 1, true);
    idml(&chapter, 1);
    let mut s = book(d.path(), &[&source, &chapter]);
    let before = std::fs::read(&chapter).unwrap();
    let result = s.execute("book.syncStyles", &json!({}));
    let after = std::fs::read(&chapter).unwrap();
    assert!(
        before == after,
        "book.syncStyles rewrote imported IDML: {result:?}; IDML readable after {}, native readable after {}",
        designcraft_idml::import_idml(&after).is_ok(),
        designcraft_format::load(&after).is_ok()
    );
}

#[test]
fn an_open_unsaved_chapter_blocks_batch_writes_before_any_files_change() {
    let d = dir("open-unsaved");
    let first = d.path().join("first.designcraft");
    let later = d.path().join("later.designcraft");
    native(&first, 9, false);
    native(&later, 5, false);
    let mut s = book(d.path(), &[&first, &later]);
    s.execute("file.open", &json!({"path":later.to_string_lossy()})).unwrap();
    s.execute("frame.create", &json!({"rect":[36,36,200,100],"content":"text","text":"Unsaved chapter edit"})).unwrap();
    let first_before = std::fs::read(&first).unwrap();
    let later_before = std::fs::read(&later).unwrap();
    let revision = s.doc().unwrap().revision;
    let result = s.execute("book.paginate", &json!({}));
    assert!(
        result.is_err(),
        "book.paginate accepted an open dirty chapter: {result:?}; first changed {}, open backing file changed {}",
        std::fs::read(&first).unwrap() != first_before,
        std::fs::read(&later).unwrap() != later_before
    );
    assert_eq!(std::fs::read(&first).unwrap(), first_before);
    assert_eq!(std::fs::read(&later).unwrap(), later_before);
    assert_eq!(s.doc().unwrap().revision, revision);
    assert!(s.doc().unwrap().is_dirty());
    let e = result.unwrap_err().to_string();
    assert!(e.contains("later.designcraft") && e.contains("close"), "{e}");
}

#[cfg(unix)]
#[test]
fn an_alias_of_an_open_chapter_blocks_all_batch_writes() {
    let d = dir("open-alias");
    let first = d.path().join("first.designcraft");
    let real = d.path().join("real.designcraft");
    let alias = d.path().join("alias.designcraft");
    native(&first, 9, false);
    native(&real, 5, false);
    std::os::unix::fs::symlink("real.designcraft", &alias).unwrap();
    let mut s = book(d.path(), &[&first, &alias]);
    s.execute("file.open", &json!({"path":real.to_string_lossy()})).unwrap();
    let first_before = std::fs::read(&first).unwrap();
    let real_before = std::fs::read(&real).unwrap();
    let result = s.execute("book.paginate", &json!({}));
    assert!(result.is_err(), "book.paginate accepted alias of open chapter: {result:?}");
    assert_eq!(std::fs::read(&first).unwrap(), first_before);
    assert_eq!(std::fs::read(&real).unwrap(), real_before);
    let e = result.unwrap_err().to_string();
    assert!(e.contains("alias.designcraft") && e.contains("close"), "{e}");
    assert!(std::fs::symlink_metadata(alias).unwrap().file_type().is_symlink());
}
#[test]
fn imported_idml_remains_a_read_only_style_source() {
    let d = dir("idml-style-source");
    let source = d.path().join("source.idml");
    let target = d.path().join("target.designcraft");
    std::fs::write(&source, designcraft_idml::export_idml(&document(1, true))).unwrap();
    native(&target, 1, false);
    let mut s = book(d.path(), &[&source, &target]);
    let before = std::fs::read(&source).unwrap();
    let result = s.execute("book.syncStyles", &json!({})).unwrap();
    assert_eq!(result["synced"], 1);
    assert_eq!(std::fs::read(&source).unwrap(), before);
    assert!(designcraft_idml::import_idml(&before).is_ok());
    let target = designcraft_format::load(&std::fs::read(target).unwrap()).unwrap();
    assert!(target.styles.paragraph.iter().any(|s| s.name == "Synthetic Book Style"));
}
#[test]
fn imported_idml_still_exports_to_book_pdf_without_source_changes() {
    let d = dir("idml-pdf");
    let chapter = d.path().join("chapter.idml");
    idml(&chapter, 1);
    let mut s = book(d.path(), &[&chapter]);
    let before = std::fs::read(&chapter).unwrap();
    let result = s.execute("book.exportPdf", &json!({})).unwrap();
    assert_eq!(result["pages"], 2);
    assert!(result["base64"].as_str().is_some_and(|s| !s.is_empty()));
    assert_eq!(std::fs::read(chapter).unwrap(), before);
}
