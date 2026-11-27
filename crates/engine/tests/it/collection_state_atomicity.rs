use designcraft_engine::Session;
use serde_json::json;

fn library_failure(command: &str) {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("library");
    std::fs::create_dir(&folder).unwrap();
    let file = folder.join("items.dclib");
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("frame.create", &json!({"rect": [0, 0, 50, 50]})).unwrap();
    s.execute("library.new", &json!({"path": file})).unwrap();
    s.execute("library.add", &json!({"name": "Keep"})).unwrap();
    let before = serde_json::to_value(&s.library).unwrap();
    let path = s.library.as_ref().unwrap().path.clone();
    let journal = s.journal.clone();
    let original_bytes = std::fs::read(&file).unwrap();
    let kept = temp.path().join("kept-library");
    std::fs::rename(&folder, &kept).unwrap();

    assert!(s.execute(command, &json!({"name": "New", "index": 0})).is_err());

    assert_eq!(before, serde_json::to_value(&s.library).unwrap(), "a failed library write must retain its original items");
    assert_eq!(path, s.library.as_ref().unwrap().path);
    assert_eq!(journal, s.journal);
    assert_eq!(original_bytes, std::fs::read(kept.join("items.dclib")).unwrap());
}

#[test]
fn failed_library_add_keeps_existing_items() {
    library_failure("library.add");
}

#[test]
fn failed_library_remove_keeps_existing_items() {
    library_failure("library.remove");
}

fn book_failure(command: &str) {
    let temp = tempfile::tempdir().unwrap();
    let mut s = Session::new();
    let mut chapters = Vec::new();
    for i in 0..3 {
        s.execute("file.new", &json!({})).unwrap();
        let path = temp.path().join(format!("chapter-{i}.designcraft"));
        s.execute("file.saveAs", &json!({"path": path})).unwrap();
        chapters.push(path);
    }
    let folder = temp.path().join("book");
    std::fs::create_dir(&folder).unwrap();
    let file = folder.join("chapters.dcbook");
    s.execute("book.new", &json!({"path": file})).unwrap();
    s.execute("book.add", &json!({"path": chapters[0]})).unwrap();
    s.execute("book.add", &json!({"path": chapters[1]})).unwrap();
    s.execute("book.styleSource", &json!({"index": 1})).unwrap();
    let before = serde_json::to_value(&s.book).unwrap();
    let path = s.book.as_ref().unwrap().path.clone();
    let journal = s.journal.clone();
    let original_bytes = std::fs::read(&file).unwrap();
    let kept = temp.path().join("kept-book");
    std::fs::rename(&folder, &kept).unwrap();
    let params = if command == "book.add" { json!({"path": chapters[2]}) } else { json!({"index": 0}) };

    assert!(s.execute(command, &params).is_err());

    assert_eq!(before, serde_json::to_value(&s.book).unwrap(), "a failed book write must retain its original chapter list and style source");
    assert_eq!(path, s.book.as_ref().unwrap().path);
    assert_eq!(journal, s.journal);
    assert_eq!(original_bytes, std::fs::read(kept.join("chapters.dcbook")).unwrap());
}

#[test]
fn failed_book_add_keeps_existing_chapters() {
    book_failure("book.add");
}

#[test]
fn failed_book_remove_keeps_existing_chapters() {
    book_failure("book.remove");
}

#[test]
fn failed_book_style_source_keeps_existing_source() {
    book_failure("book.styleSource");
}
