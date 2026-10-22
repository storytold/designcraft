//! The automation roots at the engine: the files a document names (links, data sources, book
//! chapters, packaged fonts) are read and written through the same checks as the paths a command
//! is given.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::file_access::{AutomationRoots, confine};
use crate::{Session, cmd};

const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0,
    0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// A fresh folder with `in` (the read root), `out` (the write root) and `outside`, and the roots.
fn folders(name: &str) -> (PathBuf, Arc<AutomationRoots>) {
    let base = std::env::temp_dir().join(format!("dc-engine-roots-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    for d in ["in", "out", "outside"] {
        std::fs::create_dir_all(base.join(d)).unwrap();
    }
    let roots = AutomationRoots::new(Some(&base.join("in")), Some(&base.join("out"))).unwrap().unwrap();
    (base, roots)
}

fn p(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// A document with `pic.png` placed from `dir`, so the asset is embedded and linked to the file.
fn linked_picture(dir: &Path) -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("file.place", &json!({"path": p(&dir.join("pic.png")), "x": 36, "y": 36, "width": 72})).unwrap();
    s
}

fn link_status(s: &mut Session) -> Value {
    s.execute("links.list", &json!({})).unwrap()[0]["status"].clone()
}

#[test]
fn a_link_outside_the_read_root_reads_as_missing() {
    let (base, roots) = folders("links");
    std::fs::write(base.join("outside/pic.png"), PNG).unwrap();
    std::fs::write(base.join("in/pic.png"), PNG).unwrap();
    let mut outside = linked_picture(&base.join("outside"));
    let mut inside = linked_picture(&base.join("in"));
    assert_eq!(link_status(&mut outside), "ok");
    confine(Some(&roots), || {
        assert_eq!(link_status(&mut outside), "missing", "the file is not the agent's to look at, not even to compare");
        assert_eq!(link_status(&mut inside), "ok");
        assert!(outside.execute("links.relinkFolder", &json!({"dir": p(&base.join("outside"))})).is_err());
        // A relink to a link that leads out is refused too, and the asset keeps what it had.
        let asset = outside.execute("links.list", &json!({})).unwrap()[0]["asset"].clone();
        let e = outside.execute("links.relink", &json!({"asset": asset, "path": p(&base.join("outside/pic.png"))})).unwrap_err().to_string();
        assert!(e.contains("outside the read root"), "{e}");
    });
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn idml_links_are_read_through_the_roots() {
    let (base, roots) = folders("idml");
    std::fs::write(base.join("outside/pic.png"), PNG).unwrap();
    let mut s = linked_picture(&base.join("outside"));
    // Linked, not embedded: opening the package has to read the file its link names.
    let idml = base64_idml(&mut s);
    let linked = |doc: &designcraft_doc::Document| doc.assets.values().filter(|a| !a.data.is_empty()).count();
    let package = base.join("outside/x.idml");
    let unconfined = cmd::interchange::import_idml_confined(&idml, Some(&package)).unwrap();
    assert_eq!(linked(&unconfined), 1, "without roots the link is followed");
    let confined = confine(Some(&roots), || cmd::interchange::import_idml_confined(&idml, Some(&package))).unwrap();
    assert_eq!(linked(&confined), 0, "with roots a link outside the read root is not read");
    // The same through the commands that open or place a package.
    let b64 = cmd::base64_encode(&idml);
    let mut t = Session::new();
    t.execute("file.new", &json!({})).unwrap();
    confine(Some(&roots), || {
        t.execute("file.openBytes", &json!({"name": "x.idml", "base64": b64})).unwrap();
        let doc = &t.doc().unwrap().doc;
        assert_eq!(linked(doc), 0);
        t.execute("file.place", &json!({"base64": b64, "name": "x.idml", "layoutPage": 1, "x": 0, "y": 0})).unwrap();
        assert_eq!(linked(&t.doc().unwrap().doc), 0);
    });
    let _ = std::fs::remove_dir_all(base);
}

fn base64_idml(s: &mut Session) -> Vec<u8> {
    let r = s.execute("file.exportIdml", &json!({"embedImages": false})).unwrap();
    cmd::base64_decode(r["base64"].as_str().unwrap())
}

#[test]
fn a_merge_does_not_read_image_files_outside_the_read_root() {
    let (base, roots) = folders("merge");
    std::fs::write(base.join("outside/pic.png"), PNG).unwrap();
    std::fs::write(base.join("in/data.csv"), format!("@Photo\n{}\n", base.join("outside/pic.png").display())).unwrap();
    let merge = |s: &mut Session| {
        s.execute("file.new", &json!({})).unwrap();
        s.execute("data.source.select", &json!({"path": p(&base.join("in/data.csv"))})).unwrap();
        let frame = s.execute("frame.create", &json!({"rect": [36.0, 36.0, 80.0, 80.0], "content": "graphic"})).unwrap();
        s.execute("data.placeholder.add", &json!({"field": "Photo", "item": frame["id"]})).unwrap();
        s.execute("data.merge", &json!({})).unwrap()["missingImages"].as_array().unwrap().len()
    };
    assert_eq!(merge(&mut Session::new()), 0, "without roots the picture is found");
    assert_eq!(confine(Some(&roots), || merge(&mut Session::new())), 1, "with roots it reads as missing");
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn a_data_source_path_stored_in_a_document_is_read_through_the_roots() {
    let (base, roots) = folders("source");
    std::fs::write(base.join("outside/data.csv"), "name\nTOP SECRET\n").unwrap();
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("data.source.select", &json!({"path": p(&base.join("outside/data.csv"))})).unwrap();
    confine(Some(&roots), || {
        let e = s.execute("data.source.update", &json!({})).unwrap_err().to_string();
        assert!(e.contains("outside the read root") && !e.contains("TOP SECRET"), "{e}");
    });
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn a_book_writes_its_chapters_only_inside_the_write_root() {
    let (base, roots) = folders("book");
    let chapter = base.join("in/a.designcraft");
    let second = base.join("in/b.designcraft");
    {
        // Made by another session: a book only rewrites chapters that are not open.
        let mut maker = Session::new();
        maker.execute("file.new", &json!({})).unwrap();
        maker.execute("file.save", &json!({"path": p(&chapter)})).unwrap();
        maker.execute("file.saveAs", &json!({"path": p(&second)})).unwrap();
    }
    let mut s = Session::new();
    let before = (std::fs::read(&chapter).unwrap(), std::fs::read(&second).unwrap());
    confine(Some(&roots), || {
        s.execute("book.new", &json!({"path": p(&base.join("out/b.dcbook"))})).unwrap();
        s.execute("book.add", &json!({"path": p(&chapter)})).unwrap();
        s.execute("book.add", &json!({"path": p(&second)})).unwrap();
        // Paginate and Synchronize rewrite the chapters (in the read root); Export writes `path`.
        let outside_pdf = json!({"path": p(&base.join("outside/book.pdf"))});
        for (command, params) in [("book.paginate", json!({})), ("book.syncStyles", json!({})), ("book.exportPdf", outside_pdf)] {
            let r = s.execute(command, &params);
            let e = r.unwrap_err().to_string();
            assert!(e.contains("outside the write root"), "{command}: {e}");
        }
        let e = s.execute("book.new", &json!({"path": p(&base.join("outside/b.dcbook"))})).unwrap_err().to_string();
        assert!(e.contains("outside the write root"), "{e}");
    });
    assert_eq!((std::fs::read(&chapter).unwrap(), std::fs::read(&second).unwrap()), before, "no chapter was rewritten");
    assert!(!base.join("outside/b.dcbook").exists());
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn package_and_copy_links_create_nothing_outside_the_write_root() {
    let (base, roots) = folders("package");
    std::fs::write(base.join("in/pic.png"), PNG).unwrap();
    let mut s = linked_picture(&base.join("in"));
    confine(Some(&roots), || {
        for command in ["file.package", "links.copyTo"] {
            let e = s.execute(command, &json!({"dir": p(&base.join("outside/pkg"))})).unwrap_err().to_string();
            assert!(e.contains("outside the write root"), "{command}: {e}");
            // The read root is not writable either.
            let e = s.execute(command, &json!({"dir": p(&base.join("in/pkg"))})).unwrap_err().to_string();
            assert!(e.contains("outside the write root"), "{command}: {e}");
        }
        s.execute("file.package", &json!({"dir": p(&base.join("out/pkg"))})).unwrap();
        s.execute("links.copyTo", &json!({"dir": p(&base.join("out/copies"))})).unwrap();
    });
    assert!(!base.join("outside/pkg").exists() && !base.join("in/pkg").exists());
    assert!(base.join("out/pkg/Instructions.txt").exists() && base.join("out/copies/pic.png").exists());
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn recovery_folders_named_by_a_command_are_checked() {
    let (base, roots) = folders("recovery");
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("frame.create", &json!({"rect": [10, 10, 100, 100]})).unwrap();
    confine(Some(&roots), || {
        let out = p(&base.join("outside"));
        let inside = p(&base.join("in"));
        let to_write = p(&base.join("out"));
        let e = s.execute("file.recovery.save", &json!({"dir": out})).unwrap_err().to_string();
        assert!(e.contains("outside the write root"), "{e}");
        let e = s.execute("file.recovery.save", &json!({"dir": inside})).unwrap_err().to_string();
        assert!(e.contains("outside the write root"), "{e}");
        assert!(s.execute("file.recovery.list", &json!({"dir": out})).unwrap_err().to_string().contains("outside the read root"));
        // Reopening removes what it read, so it needs both.
        assert!(s.execute("file.recovery.open", &json!({"dir": inside})).unwrap_err().to_string().contains("outside the write root"));
        assert!(s.execute("file.recovery.open", &json!({"dir": to_write})).unwrap_err().to_string().contains("outside the read root"));
        assert_eq!(s.execute("file.recovery.save", &json!({"dir": to_write})).unwrap()["saved"], 1);
    });
    assert!(std::fs::read_dir(base.join("outside")).unwrap().next().is_none());
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn fonts_beside_a_document_outside_the_read_root_are_not_loaded() {
    let (base, roots) = folders("fonts");
    std::fs::create_dir_all(base.join("outside/Document Fonts")).unwrap();
    std::fs::write(base.join("outside/Document Fonts/x.ttf"), b"not a font").unwrap();
    let mut d = designcraft_doc::Document::new(&designcraft_doc::build::NewDocument::default());
    let (_, faces, warnings) = confine(Some(&roots), || cmd::load_document_fonts(&mut d, &p(&base.join("outside/doc.designcraft"))));
    assert_eq!(faces, 0);
    assert!(warnings.iter().any(|w| w.contains("outside the read root")), "{warnings:?}");
    let (_, _, warnings) = cmd::load_document_fonts(&mut d, &p(&base.join("outside/doc.designcraft")));
    assert!(warnings.iter().any(|w| w.contains("x.ttf")), "without roots the folder is read and the bad font reported: {warnings:?}");
    let _ = std::fs::remove_dir_all(base);
}

#[cfg(unix)]
#[test]
fn a_font_file_that_links_out_of_the_read_root_is_skipped() {
    let (base, roots) = folders("fontlink");
    std::fs::create_dir_all(base.join("in/Document Fonts")).unwrap();
    std::fs::write(base.join("outside/secret.ttf"), b"not a font").unwrap();
    std::os::unix::fs::symlink(base.join("outside/secret.ttf"), base.join("in/Document Fonts/link.ttf")).unwrap();
    let mut d = designcraft_doc::Document::new(&designcraft_doc::build::NewDocument::default());
    let doc = p(&base.join("in/doc.designcraft"));
    let (_, _, warnings) = confine(Some(&roots), || cmd::load_document_fonts(&mut d, &doc));
    assert!(warnings.iter().any(|w| w.contains("outside the read root")), "{warnings:?}");
    assert!(!warnings.iter().any(|w| w.contains("not a font")), "the file was opened: {warnings:?}");
    let (_, _, warnings) = cmd::load_document_fonts(&mut d, &doc);
    assert!(warnings.iter().any(|w| w.contains("link.ttf")), "without roots the link is followed: {warnings:?}");
    let _ = std::fs::remove_dir_all(base);
}
