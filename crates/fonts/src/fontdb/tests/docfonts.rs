//! Document fonts: the fonts in a `Document Fonts` folder beside a document, loaded for that
//! document alone (its scope) ahead of the installed fonts of the same name; damaged, oversized
//! and surplus files are skipped.

use std::path::{Path, PathBuf};

use crate::fontdb::{Arc, DOCUMENT_FONTS_FOLDER, FALLBACK_FAMILY, FontDb, FontSource, MAX_DOCUMENT_FONT_BYTES, MAX_DOCUMENT_FONT_FILES};
use crate::testing::{font_with, font_with_names, font_with_wws, with_fs_type};

/// An empty temporary folder unique to `name` and this process.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dc-docfonts-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_font(dir: &Path, file: &str, family: &str, chars: &[char]) -> PathBuf {
    let path = dir.join(file);
    std::fs::write(&path, font_with(family, chars).unwrap()).unwrap();
    path
}

#[test]
fn document_fonts_load_ahead_of_installed_ones() {
    let root = temp_dir("precedence");
    let system = root.join("system");
    let folder = root.join(DOCUMENT_FONTS_FOLDER);
    std::fs::create_dir_all(&system).unwrap();
    std::fs::create_dir_all(&folder).unwrap();
    write_font(&system, "installed.ttf", "DocFont Precedence", &['a']);
    let file = write_font(&folder, "doc.ttf", "DocFont Precedence", &['b']);

    let db = FontDb::with_font_dirs(vec![system]);
    // The installed face is loaded first (the document was composed before its folder appeared).
    let installed = db.face("DocFont Precedence", "Regular");
    assert!(installed.covers('a') && matches!(installed.source, FontSource::Installed(_)), "{:?}", installed.source);

    let r = db.load_document_fonts(&folder, &|_| Ok(()));
    assert_eq!((r.faces, r.skipped.len()), (1, 0), "{:?}", r.skipped);
    let face = db.scoped(r.scope).face("DocFont Precedence", "Regular");
    assert!(face.covers('b'), "the document's face wins");
    assert_eq!(face.source, FontSource::Document(file.clone()));
    assert_eq!(face.source.path(), Some(file.as_path()));
    assert!(db.face("DocFont Precedence", "Regular").covers('a'), "outside the document the installed face stays");

    // Loading the folder again (another document beside it) shares the face, in a scope of its own.
    let again = db.load_document_fonts(&folder, &|_| Ok(()));
    assert_eq!((again.faces, again.skipped.len()), (1, 0));
    assert_ne!(again.scope, r.scope);
    assert!(Arc::ptr_eq(&db.scoped(again.scope).face("DocFont Precedence", "Regular"), &face));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn damaged_oversized_and_other_files_are_skipped() {
    let folder = temp_dir("skipped");
    write_font(&folder, "good.otf", "DocFont Good", &['g']);
    std::fs::write(folder.join("broken.ttf"), b"not a font at all").unwrap();
    // Sparse: takes no disk space, but its length is over the limit.
    std::fs::File::create(folder.join("huge.ttc")).unwrap().set_len(MAX_DOCUMENT_FONT_BYTES + 1).unwrap();
    std::fs::write(folder.join("readme.txt"), b"fonts for the brochure").unwrap();
    std::fs::create_dir_all(folder.join("nested.ttf")).unwrap();
    write_font(&folder.join("nested.ttf"), "inner.ttf", "DocFont Nested", &['n']);

    let db = FontDb::with_font_dirs(vec![]);
    let r = db.load_document_fonts(&folder, &|_| Ok(()));
    assert_eq!(r.faces, 1);
    assert!(db.scoped(r.scope).has_family("DocFont Good"));
    assert!(!db.has_family("DocFont Good"), "the document's alone");
    assert!(!db.scoped(r.scope).has_family("DocFont Nested"), "subfolders aren't read");
    assert_eq!(r.skipped.len(), 2, "{:?}", r.skipped);
    assert!(r.skipped.iter().any(|s| s.contains("broken.ttf")), "{:?}", r.skipped);
    assert!(r.skipped.iter().any(|s| s.contains("huge.ttc") && s.contains(&format!("{} MB", MAX_DOCUMENT_FONT_BYTES >> 20))), "{:?}", r.skipped);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn the_number_of_files_read_is_capped() {
    let folder = temp_dir("cap");
    for i in 0..MAX_DOCUMENT_FONT_FILES + 5 {
        std::fs::write(folder.join(format!("{i:04}.ttf")), b"junk").unwrap();
    }
    // Sorts after every junk file, so it is past the cap.
    write_font(&folder, "zz-real.ttf", "DocFont Past Cap", &['z']);

    let db = FontDb::with_font_dirs(vec![]);
    let r = db.load_document_fonts(&folder, &|_| Ok(()));
    assert_eq!((r.faces, r.scope), (0, 0));
    assert!(!db.scoped(r.scope).has_family("DocFont Past Cap"));
    assert_eq!(r.skipped.len(), MAX_DOCUMENT_FONT_FILES + 1, "one note for each file read, one for the rest");
    assert!(r.skipped.last().is_some_and(|s| s.contains("6 more") && s.contains("200")), "{:?}", r.skipped.last());
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn large_cjk_collections_are_within_the_limit() {
    // macOS's Songti collection is 67 MB; full Noto/Source Han CJK collections are larger.
    let folder = temp_dir("cjk-size");
    std::fs::File::create(folder.join("cjk.ttc")).unwrap().set_len(200 << 20).unwrap();
    let r = FontDb::with_font_dirs(vec![]).load_document_fonts(&folder, &|_| Ok(()));
    assert_eq!(r.skipped.len(), 1, "{:?}", r.skipped);
    assert!(!r.skipped[0].contains("larger than"), "skipped for not being a font, not for its size: {:?}", r.skipped);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_missing_folder_loads_nothing() {
    let db = FontDb::with_font_dirs(vec![]);
    let r = db.load_document_fonts(&std::env::temp_dir().join("dc-docfonts-no-such-folder").join(DOCUMENT_FONTS_FOLDER), &|_| Ok(()));
    assert_eq!((r.faces, r.scope, r.skipped.len()), (0, 0, 0));
}

#[test]
fn each_scope_sees_its_own_fonts_only() {
    const FAMILY: &str = "DocFont Scope";
    let (a, b) = (temp_dir("scope-a"), temp_dir("scope-b"));
    write_font(&a, "a.ttf", FAMILY, &['a']);
    write_font(&b, "b.ttf", FAMILY, &['b', '\u{E123}']);
    let db = FontDb::with_font_dirs(vec![]);
    let (ra, rb) = (db.load_document_fonts(&a, &|_| Ok(())), db.load_document_fonts(&b, &|_| Ok(())));
    let (in_a, in_b) = (db.scoped(ra.scope), db.scoped(rb.scope));

    assert!(in_a.face(FAMILY, "Regular").covers('a') && in_b.face(FAMILY, "Regular").covers('b'));
    assert!(in_a.families().iter().any(|f| f == FAMILY) && in_a.styles(FAMILY) == ["Regular"]);
    assert!(!db.has_family(FAMILY) && !db.families().iter().any(|f| f == FAMILY), "no document's fonts are shared");
    assert!(!db.scoped(0).has_family(FAMILY));
    // Fallback finds a character in the document's own fonts, never another document's.
    let exclude = in_a.face(FALLBACK_FAMILY, "Regular").id();
    assert!(in_a.fallback_for('\u{E123}', exclude, None).is_none());
    assert!(in_b.fallback_for('\u{E123}', exclude, None).is_some_and(|f| f.family == FAMILY));
    let _ = std::fs::remove_dir_all(&a);
    let _ = std::fs::remove_dir_all(&b);
}

#[test]
fn a_closed_scope_finds_nothing_and_reopening_reuses_the_faces() {
    const FAMILY: &str = "DocFont Reopened";
    let folder = temp_dir("reopen");
    let file = write_font(&folder, "r.ttf", FAMILY, &['r']);
    let db = FontDb::with_font_dirs(vec![]);
    let r = db.load_document_fonts(&folder, &|_| Ok(()));
    let face = db.scoped(r.scope).face(FAMILY, "Regular");
    db.close_scope(r.scope);
    assert!(!db.scoped(r.scope).has_family(FAMILY), "closed");

    let again = db.load_document_fonts(&folder, &|_| Ok(()));
    assert!(Arc::ptr_eq(&db.scoped(again.scope).face(FAMILY, "Regular"), &face), "an unchanged file isn't read again");
    // A file changed since is read again.
    std::fs::write(&file, font_with(FAMILY, &['r', 's']).unwrap()).unwrap();
    let changed = db.load_document_fonts(&folder, &|_| Ok(()));
    let new_face = db.scoped(changed.scope).face(FAMILY, "Regular");
    assert!(new_face.covers('s') && new_face.id() != face.id());
    assert!(!db.scoped(again.scope).face(FAMILY, "Regular").covers('s'), "the document open before keeps what it opened with");
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn restricted_licence_reads_the_embedding_bits() {
    let db = FontDb::with_font_dirs(vec![]);
    for (fs_type, restricted) in [(0x0000, false), (0x0002, true), (0x0006, false), (0x000A, false), (0x0008, false), (0x0302, true)] {
        let family = format!("DocFont Licence {fs_type:04x}");
        assert_eq!(db.add_font(with_fs_type(font_with(&family, &['l']).unwrap(), fs_type).unwrap()), 1);
        assert_eq!(db.face(&family, "Regular").restricted_licence(), restricted, "fsType {fs_type:#06x}");
    }
    assert!(!db.face(FALLBACK_FAMILY, "Regular").restricted_licence(), "the bundled fonts are OFL");
    assert_eq!(db.face(FALLBACK_FAMILY, "Regular").source, FontSource::Bundled);
}

#[test]
fn the_access_check_vets_the_folder_and_every_file_before_it_is_opened() {
    let folder = temp_dir("access");
    write_font(&folder, "kept.ttf", "DocFont Kept", &['k']);
    write_font(&folder, "refused.ttf", "DocFont Refused", &['r']);
    let db = FontDb::with_font_dirs(vec![]);
    let asked = std::sync::Mutex::new(Vec::new());
    let allow = |p: &Path| {
        asked.lock().unwrap().push(p.to_path_buf());
        if p.ends_with("refused.ttf") { Err("refused.ttf: outside the read root".to_string()) } else { Ok(()) }
    };
    let r = db.load_document_fonts(&folder, &allow);
    assert_eq!(r.faces, 1, "{:?}", r.skipped);
    assert_eq!(r.skipped, vec!["refused.ttf: outside the read root".to_string()]);
    let asked = asked.into_inner().unwrap();
    assert_eq!(asked.first().map(PathBuf::as_path), Some(folder.as_path()), "the folder is asked about first");
    assert!(asked.iter().any(|p| p.ends_with("kept.ttf")) && asked.iter().any(|p| p.ends_with("refused.ttf")));

    // A refused folder is not even listed.
    let r = db.load_document_fonts(&folder, &|_| Err("no".to_string()));
    assert_eq!((r.faces, r.scope, r.skipped), (0, 0, vec!["no".to_string()]));
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn optical_size_face_is_found_by_its_wws_names() {
    let folder = temp_dir("wws");
    let bytes = font_with_wws("DocFont Kepler", "Light Display", "DocFont Kepler Display", "Light", &['k']).unwrap();
    std::fs::write(folder.join("light-display.otf"), bytes).unwrap();
    let db = FontDb::with_font_dirs(vec![]);
    let r = db.load_document_fonts(&folder, &|_| Ok(()));
    assert_eq!((r.faces, r.skipped.len()), (1, 0), "{:?}", r.skipped);
    let fonts = db.scoped(r.scope);
    assert!(fonts.has_family("DocFont Kepler Display"), "the WWS family counts as available");
    assert_eq!(fonts.face("DocFont Kepler Display", "Light").style, "Light Display");
    // `font.list` checks the style against these: "Light" isn't a missing style.
    assert_eq!(fonts.styles("DocFont Kepler Display"), ["Light"]);
    // The typographic names still find it.
    assert!(fonts.has_family("DocFont Kepler"));
    assert_eq!(fonts.face("DocFont Kepler", "Light Display").style, "Light Display");
    assert_eq!(fonts.styles("DocFont Kepler"), ["Light Display"]);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_wws_family_that_is_also_the_legacy_family_finds_the_optical_size_face() {
    // Optical-size faces usually carry their WWS family as the legacy family (name ID 1) too.
    let folder = temp_dir("wws-legacy");
    let display =
        [(1, "DocFont Kepler Display"), (2, "Regular"), (16, "DocFont Kepler"), (17, "Light Display"), (21, "DocFont Kepler Display"), (22, "Light")];
    let text = [(1, "DocFont Kepler Light"), (2, "Regular"), (16, "DocFont Kepler"), (17, "Light")];
    std::fs::write(folder.join("light-display.otf"), font_with_names(&display, &['k']).unwrap()).unwrap();
    std::fs::write(folder.join("light.otf"), font_with_names(&text, &['k']).unwrap()).unwrap();
    let db = FontDb::with_font_dirs(vec![]);
    let r = db.load_document_fonts(&folder, &|_| Ok(()));
    assert_eq!((r.faces, r.skipped.len()), (2, 0), "{:?}", r.skipped);
    let fonts = db.scoped(r.scope);
    assert_eq!(fonts.face("DocFont Kepler Display", "Light").style, "Light Display", "the display face, not the text size");
    assert_eq!(fonts.styles("DocFont Kepler Display"), ["Light"]);
    assert!(fonts.has_style("DocFont Kepler Display", "Light"));
    assert_eq!(fonts.face("DocFont Kepler", "Light").style, "Light");
    let _ = std::fs::remove_dir_all(&folder);
}
