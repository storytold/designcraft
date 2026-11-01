use super::tests::doc_with;
use super::*;
use std::sync::Arc;

struct Fixture {
    dir: tempfile::TempDir,
    session: Session,
}

impl Fixture {
    fn new() -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let chapters: [(&str, &[(&str, &str)]); 4] = [
            ("toc.designcraft", &[("Local", "Local Head")]),
            ("z.designcraft", &[("Zulu", "Book Head"), ("Zulu child", "Remote Only")]),
            ("a.designcraft", &[("Alpha", "Book Head")]),
            ("empty.designcraft", &[]),
        ];
        for (name, paras) in chapters {
            let (s, _) = doc_with(paras);
            std::fs::write(dir.path().join(name), designcraft_format::save(&s.doc()?.doc)?)?;
        }
        // Deliberately relative paths, and manifest order different from filename/alphabetical order.
        std::fs::write(
            dir.path().join("test.dcbook"),
            serde_json::to_vec(&json!({"documents": ["toc.designcraft", "z.designcraft", "a.designcraft", "empty.designcraft"]}))?,
        )?;
        let mut session = Session::new();
        session.execute("book.open", &json!({"path": dir.path().join("test.dcbook")}))?;
        session.execute("book.paginate", &json!({}))?;
        session.execute("file.open", &json!({"path": dir.path().join("toc.designcraft")}))?;
        Ok(Self { dir, session })
    }

    fn params(&self) -> Value {
        json!({"scope": "entireBook", "page": 2, "entries": [
            {"style": "Local Head", "level": 1},
            {"style": "Book Head", "level": 2},
            {"style": "Remote Only", "level": 3},
        ]})
    }

    fn read(&self, name: &str) -> std::result::Result<Document, Box<dyn std::error::Error>> {
        Ok(designcraft_format::load(&std::fs::read(self.dir.path().join(name))?)?)
    }
}

#[test]
fn entire_book_collects_in_manifest_order_with_local_ids_levels_and_remote_styles() {
    let mut f = Fixture::new().unwrap();
    let first = f.read("z.designcraft").unwrap();
    let second = f.read("a.designcraft").unwrap();
    assert_eq!(first.stories.keys().next(), second.stories.keys().next(), "synthetic chapters deliberately reuse local IDs");
    let local = f.session.execute("toc.styles", &json!({})).unwrap();
    assert_eq!(local["bookAvailable"], true);
    assert!(!local["styles"].as_array().unwrap().contains(&json!("Remote Only")));
    let all = f.session.execute("toc.styles", &json!({"scope": "entireBook"})).unwrap();
    assert!(all["styles"].as_array().unwrap().contains(&json!("Remote Only")));
    let params = f.params();
    let entries = f.session.execute("toc.entries", &params_for_query(&params)).unwrap();
    assert_eq!(
        entries
            .as_array()
            .unwrap()
            .iter()
            .map(|e| (e["text"].as_str().unwrap(), e["level"].as_u64().unwrap(), e["page"].as_str().unwrap()))
            .collect::<Vec<_>>(),
        [("Local", 1, "1"), ("Zulu", 2, "3"), ("Zulu child", 3, "3"), ("Alpha", 2, "5")]
    );
    assert_ne!(entries[1]["document"], entries[3]["document"]);
    // Active source is read from memory, including an unsaved heading edit.
    let source = f.session.doc().unwrap().doc.stories.keys().next().unwrap().0;
    f.session.execute("story.replaceRange", &json!({"story": source, "start": 0, "end": 5, "text": "Live"})).unwrap();
    let backing = std::fs::read(f.dir.path().join("toc.designcraft")).unwrap();
    let r = f.session.execute("toc.generate", &params).unwrap();
    assert_eq!(r["entries"], 4);
    let toc = f.session.doc().unwrap().doc.toc.clone().unwrap();
    assert_eq!(toc.scope, TocScope::EntireBook);
    let text = f.session.doc().unwrap().doc.story(toc.story).unwrap().text.clone();
    assert!(text.starts_with("Contents\nLive\t1\nZulu\t3\nZulu child\t3\nAlpha\t5"), "{text}");
    let paras = &f.session.doc().unwrap().doc.story(toc.story).unwrap().paras;
    assert_eq!(paras.iter().map(|p| p.style.as_str()).collect::<Vec<_>>(), ["TOC Title", "TOC Level 1", "TOC Level 2", "TOC Level 3", "TOC Level 2"]);
    assert_eq!(std::fs::read(f.dir.path().join("toc.designcraft")).unwrap(), backing, "no disk write behind the active model");
    let mut changed = f.read("a.designcraft").unwrap();
    let sid = *changed.stories.keys().next().unwrap();
    changed.story_mut(sid).unwrap().replace(0..5, "Changed");
    std::fs::write(f.dir.path().join("a.designcraft"), designcraft_format::save(&changed).unwrap()).unwrap();
    f.session.execute("toc.update", &json!({})).unwrap();
    assert!(f.session.doc().unwrap().doc.story(toc.story).unwrap().text.contains("Changed\t5"));
    f.session.execute("file.save", &json!({})).unwrap();
    f.session.execute("file.close", &json!({})).unwrap();
    f.session.execute("file.open", &json!({"path": f.dir.path().join("toc.designcraft")})).unwrap();
    assert_eq!(f.session.doc().unwrap().doc.toc.as_ref().unwrap().scope, TocScope::EntireBook);
    f.session.execute("toc.update", &json!({})).unwrap();
}

fn params_for_query(params: &Value) -> Value {
    json!({"scope": params["scope"], "entries": params["entries"]})
}

#[test]
fn entire_book_rejects_missing_membership_invalid_scope_and_global_sorting() {
    let (mut s, _) = doc_with(&[("Local", "Head")]);
    let p = json!({"scope": "entireBook", "styles": ["Head"]});
    for command in ["toc.generate", "toc.entries"] {
        let before = s.doc().unwrap().doc.clone();
        assert!(s.execute(command, &p).unwrap_err().to_string().contains("Book"));
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    }
    assert!(s.execute("toc.entries", &json!({"styles": ["Head"], "scope": "typo"})).is_err());
    let mut f = Fixture::new().unwrap();
    f.session.execute("file.new", &json!({})).unwrap();
    f.session.execute("file.saveAs", &json!({"path": f.dir.path().join("outsider.designcraft")})).unwrap();
    assert!(f.session.execute("toc.entries", &p).unwrap_err().to_string().contains("not a member"));
    f.session.set_active(0);
    let sorted = json!({"scope": "entireBook", "entries": [{"style": "Book Head", "sort": true}]});
    assert!(f.session.execute("toc.entries", &sorted).unwrap_err().to_string().contains("alphabetical"));
    f.session.execute("book.close", &json!({})).unwrap();
    assert!(f.session.execute("toc.styles", &json!({"scope": "entireBook"})).is_err());
}

#[test]
fn entire_book_refuses_other_open_chapters_but_refreshes_stale_numbering() {
    let mut f = Fixture::new().unwrap();
    let path = f.dir.path().join("a.designcraft");
    let backing = std::fs::read(&path).unwrap();
    f.session.execute("file.open", &json!({"path": path})).unwrap();
    f.session.execute("frame.create", &json!({"rect": [36,36,300,200], "content": "text", "text": "unsaved"})).unwrap();
    f.session.set_active(0);
    let before = f.session.doc().unwrap().doc.clone();
    let params = f.params();
    assert!(f.session.execute("toc.generate", &params).unwrap_err().to_string().contains("Close the other"));
    assert!(Arc::ptr_eq(&before, &f.session.doc().unwrap().doc));
    assert_eq!(std::fs::read(&path).unwrap(), backing);
    // Existing disk-write protection stays intact too.
    assert!(f.session.execute("book.paginate", &json!({})).is_err());
    f.session.set_active(1);
    f.session.execute("file.close", &json!({})).unwrap();
    let mut chapter = f.read("a.designcraft").unwrap();
    chapter.sections.first_mut().unwrap().start_number = Some(1);
    std::fs::write(&path, designcraft_format::save(&chapter).unwrap()).unwrap();
    f.session.execute("toc.generate", &params).unwrap();
    assert!(!Arc::ptr_eq(&before, &f.session.doc().unwrap().doc));
}

#[test]
fn entire_book_layout_guard_rolls_back_overset_and_page_growth() {
    let mut f = Fixture::new().unwrap();
    let mut params = f.params();
    params["rect"] = json!([36, 36, 80, 40]);
    let old = f.session.doc().unwrap().doc.clone();
    let revision = f.session.doc().unwrap().revision;
    let undo = f.session.doc().unwrap().history.undo.len();
    assert!(f.session.execute("toc.generate", &params).unwrap_err().to_string().contains("does not fit"));
    assert!(Arc::ptr_eq(&old, &f.session.doc().unwrap().doc));
    assert_eq!(f.session.doc().unwrap().revision, revision);
    assert_eq!(f.session.doc().unwrap().history.undo.len(), undo);
    let params = f.params();
    f.session.execute("toc.generate", &params).unwrap();
    let original = (*f.session.doc().unwrap().doc).clone();
    let mut changed = original.clone();
    changed.insert_pages(None, 1, None).unwrap();
    assert!(validate_book_layout(&original, &changed).unwrap_err().to_string().contains("outside the bounded Book stabilization path"));
    let mut overset = original.clone();
    let sid = overset.toc.as_ref().unwrap().story;
    let story = overset.story_mut(sid).unwrap();
    story.replace(0..story.len(), &"Contents ".repeat(5000));
    assert!(validate_book_layout(&original, &overset).unwrap_err().to_string().contains("overset outside the bounded Book stabilization path"));
}

#[test]
fn explicit_book_scope_exports_preference_without_activating_import() {
    let mut f = Fixture::new().unwrap();
    f.session.execute("toc.generate", &json!({"styles":["Local Head"],"page":2})).unwrap();
    f.session
        .edit(|d, _| {
            d.toc.as_mut().unwrap().idml_include_book_documents = Some(false);
            Ok(())
        })
        .unwrap();
    let params = f.params();
    let result = f.session.execute("toc.generate", &params).unwrap();
    assert!((2..=4).contains(&result["passes"].as_u64().unwrap()));
    assert_eq!(result["pendingSave"], true);
    let toc = f.session.doc().unwrap().doc.toc.as_ref().unwrap();
    assert_eq!(toc.scope, TocScope::EntireBook);
    assert_eq!(toc.idml_include_book_documents, Some(false));
    for scope in [TocScope::CurrentDocument, TocScope::EntireBook] {
        let mut d = (*f.session.doc().unwrap().doc).clone();
        d.toc.as_mut().unwrap().scope = scope;
        let bytes = designcraft_idml::export_idml(&d);
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
        let mut styles = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("Resources/Styles.xml").unwrap(), &mut styles).unwrap();
        assert!(styles.contains(if scope == TocScope::EntireBook { "IncludeBookDocuments=\"true\"" } else { "IncludeBookDocuments=\"false\"" }));
        let back = designcraft_idml::import_idml(&bytes).unwrap();
        assert_eq!(back.toc.as_ref().unwrap().scope, TocScope::CurrentDocument);
        assert_eq!(back.toc.as_ref().unwrap().idml_include_book_documents, Some(scope == TocScope::EntireBook));
    }
}

#[test]
fn entire_book_routes_growth_through_bounded_smart_reflow() {
    let mut f = Fixture::new().unwrap();
    let mut params = f.params();
    params["rect"] = json!([36, 36, 220, 220]);
    f.session.execute("toc.generate", &params).unwrap();
    f.session
        .edit(|d, _| {
            let toc = d.toc.as_mut().unwrap();
            toc.title = "A long contents title ".repeat(45);
            d.settings.primary_story = Some(toc.story);
            Ok(())
        })
        .unwrap();
    let before = f.session.doc().unwrap().doc.clone();
    let revision = f.session.doc().unwrap().revision;
    let undo = f.session.doc().unwrap().history.undo.len();
    let result = f.session.execute("toc.update", &json!({}));
    assert!(result.is_ok(), "{result:?}");
    assert!(f.session.doc().unwrap().doc.page_count() > before.page_count());
    assert!(f.session.doc().unwrap().revision > revision);
    assert_eq!(f.session.doc().unwrap().history.undo.len(), undo + 1);
    assert!(validate_book_layout(&before, &f.session.doc().unwrap().doc).is_err());
}

#[test]
fn book_membership_survives_a_missing_earlier_chapter_but_collection_stays_strict() {
    let mut f = Fixture::new().unwrap();
    f.session.book.as_mut().unwrap().documents.insert(0, "missing-cover.designcraft".into());
    f.session.execute("file.close", &json!({})).unwrap();
    f.session.execute("book.openDocument", &json!({"index":1})).unwrap();
    assert_eq!(super::super::book::active_member(&f.session).unwrap(), 1);
    let info = f.session.execute("toc.styles", &json!({})).unwrap();
    assert_eq!(info["bookAvailable"], true, "another missing chapter does not make the active member unrelated");
    assert!(info["bookError"].is_null());
    let before = f.session.doc().unwrap().doc.clone();
    let params = f.params();
    let error = f.session.execute("toc.entries", &params_for_query(&params)).unwrap_err().to_string();
    assert!(error.contains("missing-cover.designcraft"), "{error}");
    assert!(f.session.execute("toc.styles", &json!({"scope":"entireBook"})).is_err(), "no incomplete source list on unreadable Books");
    assert!(Arc::ptr_eq(&before, &f.session.doc().unwrap().doc));
}

#[test]
fn book_panel_open_command_resolves_relative_paths_and_independent_canonical_open_matches() {
    let mut f = Fixture::new().unwrap();
    f.session.execute("file.close", &json!({})).unwrap();
    f.session.book.as_mut().unwrap().documents[0] = "./sub/../toc.designcraft".into();
    std::fs::create_dir(f.dir.path().join("sub")).unwrap();
    f.session.execute("book.openDocument", &json!({"index":0})).unwrap();
    let opened = f.session.doc().unwrap().path.as_deref().unwrap();
    assert!(std::path::Path::new(opened).is_absolute());
    let expected = std::fs::canonicalize(f.dir.path().join("toc.designcraft")).unwrap();
    assert_eq!(std::fs::canonicalize(opened).unwrap(), expected);
    assert_eq!(f.session.execute("toc.styles", &json!({})).unwrap()["bookAvailable"], true);
    assert_eq!(f.session.execute("layout.chapterNumbering", &json!({})).unwrap()["book"], "test");
    f.session.execute("file.close", &json!({})).unwrap();
    f.session.execute("file.open", &json!({"path":expected})).unwrap();
    assert_eq!(f.session.execute("toc.styles", &json!({})).unwrap()["bookAvailable"], true);
    assert!(f.session.execute("book.openDocument", &json!({"index":999})).is_err());
}

#[cfg(windows)]
#[test]
fn book_membership_normalizes_windows_case_separators_and_dot_segments() {
    let mut f = Fixture::new().unwrap();
    f.session.execute("file.close", &json!({})).unwrap();
    let path = f.dir.path().join("toc.designcraft").to_string_lossy().replace('\\', "/").to_lowercase();
    f.session.execute("file.open", &json!({"path":path})).unwrap();
    f.session.book.as_mut().unwrap().documents[0] = ".\\TOC.DESIGNCRAFT".into();
    assert_eq!(f.session.execute("toc.styles", &json!({})).unwrap()["bookAvailable"], true);
}

#[test]
fn book_membership_does_not_relax_duplicate_or_open_alias_protections() {
    let mut f = Fixture::new().unwrap();
    f.session.book.as_mut().unwrap().documents.push("./toc.designcraft".into());
    let info = f.session.execute("toc.styles", &json!({})).unwrap();
    assert_eq!(info["bookAvailable"], false);
    assert!(info["bookError"].as_str().unwrap().contains("duplicate"));
    let params = f.params();
    assert!(f.session.execute("toc.entries", &params_for_query(&params)).unwrap_err().to_string().contains("duplicate"));
    f.session.book.as_mut().unwrap().documents.pop();
    let alias = f.dir.path().join("./a.designcraft");
    f.session.execute("file.open", &json!({"path":alias})).unwrap();
    f.session.set_active(0);
    assert!(f.session.execute("toc.entries", &params_for_query(&params)).unwrap_err().to_string().contains("save and close"));
    f.session.execute("file.new", &json!({})).unwrap();
    f.session.execute("file.saveAs", &json!({"path":f.dir.path().join("outsider.designcraft")})).unwrap();
    assert_eq!(f.session.execute("toc.styles", &json!({})).unwrap()["bookAvailable"], false);
}

#[test]
fn reordered_manifest_requires_numbering_then_drives_existing_toc_traversal() {
    let mut f = Fixture::new().unwrap();
    let params = params_for_query(&f.params());
    let chapter_bytes =
        ["toc.designcraft", "z.designcraft", "a.designcraft", "empty.designcraft"].map(|n| std::fs::read(f.dir.path().join(n)).unwrap());
    f.session.execute("book.move", &json!({"from":2,"to":1})).unwrap();
    for (name, before) in ["toc.designcraft", "z.designcraft", "a.designcraft", "empty.designcraft"].iter().zip(chapter_bytes) {
        assert_eq!(std::fs::read(f.dir.path().join(name)).unwrap(), before, "move never writes chapters");
    }
    let error = f.session.execute("toc.entries", &params).unwrap_err().to_string();
    assert!(error.contains("Update Numbering"), "{error}");
    assert!(f.session.execute("book.paginate", &json!({})).is_err(), "open chapter protection remains intact");
    assert!(f.session.book.as_ref().unwrap().numbering_needs_update);
    f.session.execute("file.close", &json!({})).unwrap();
    f.session.execute("book.paginate", &json!({})).unwrap();
    assert!(!f.session.book.as_ref().unwrap().numbering_needs_update);
    f.session.execute("book.close", &json!({})).unwrap();
    f.session.execute("book.open", &json!({"path": f.dir.path().join("test.dcbook")})).unwrap();
    assert!(!f.session.book.as_ref().unwrap().numbering_needs_update);
    f.session.execute("book.openDocument", &json!({"index":0})).unwrap();
    let entries = f.session.execute("toc.entries", &params).unwrap();
    let labels = entries.as_array().unwrap().iter().map(|e| (e["text"].as_str().unwrap(), e["page"].as_str().unwrap())).collect::<Vec<_>>();
    assert_eq!(labels, [("Local", "1"), ("Alpha", "3"), ("Zulu", "5"), ("Zulu child", "5")]);
}

#[path = "book_fixture_tests.rs"]
pub(crate) mod fixtures;

#[test]
fn entire_book_rejects_document_local_bookmark_destinations() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let before = serde_json::to_value(&s.doc().unwrap().doc).unwrap();
    // Validate on the scratch configuration path used by active Book generation.
    let error = configure_book(&mut s, &json!({"scope":"entireBook","styles":["Head"],"createBookmarks":true})).unwrap_err();
    assert!(error.to_string().contains("destinations span multiple documents"), "{error}");
    assert_eq!(serde_json::to_value(&s.doc().unwrap().doc).unwrap(), before);
    let mut d = fixtures::toc_document(300.0, 500.0, true, " ");
    let mut toc = d.toc.clone().unwrap();
    toc.create_bookmarks = true;
    let before = serde_json::to_value(&d).unwrap();
    assert!(fill(&mut d, &toc, None, Some(vec![])).is_err());
    assert_eq!(serde_json::to_value(&d).unwrap(), before);
}

/// An imported preference survives actual commands, native Save/reopen, and IDML re-export
/// without authorizing Book execution or raising the native capability requirement.
#[test]
fn imported_idml_book_preference_survives_local_commands_and_native_save() {
    use std::io::Read;
    let (mut source, page) = doc_with(&[("Heading", "Head")]);
    source.execute("toc.generate", &json!({"styles":["Head"],"page":page})).unwrap();
    let dir = tempfile::tempdir().unwrap();
    for preference in [false, true] {
        let mut document = (*source.doc().unwrap().doc).clone();
        document.toc.as_mut().unwrap().scope = if preference { TocScope::EntireBook } else { TocScope::CurrentDocument };
        let idml = designcraft_idml::export_idml(&document);
        let input = dir.path().join(format!("input-{preference}.idml"));
        std::fs::write(&input, idml).unwrap();
        let mut session = Session::new();
        session.execute("file.openIdml", &json!({"path":input})).unwrap();
        assert!(session.book.is_none());
        let toc = session.doc().unwrap().doc.toc.as_ref().unwrap();
        assert_eq!(toc.scope, TocScope::CurrentDocument);
        assert_eq!(toc.idml_include_book_documents, Some(preference));
        session.execute("toc.update", &json!({})).unwrap();
        session.execute("toc.generate", &json!({"styles":["Head"],"scope":"currentDocument"})).unwrap();
        let path = dir.path().join(format!("saved-{preference}.designcraft"));
        session.execute("file.saveAs", &json!({"path":path})).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut meta = String::new();
        zip.by_name("meta.json").unwrap().read_to_string(&mut meta).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&meta).unwrap()["version"], 2);
        session.execute("file.close", &json!({})).unwrap();
        session.execute("file.open", &json!({"path":path})).unwrap();
        session.execute("toc.update", &json!({})).unwrap();
        let toc = session.doc().unwrap().doc.toc.as_ref().unwrap();
        assert_eq!(toc.scope, TocScope::CurrentDocument);
        assert_eq!(toc.idml_include_book_documents, Some(preference));
        let exported = designcraft_idml::export_idml(&session.doc().unwrap().doc);
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&exported)).unwrap();
        let mut styles = String::new();
        zip.by_name("Resources/Styles.xml").unwrap().read_to_string(&mut styles).unwrap();
        assert!(styles.contains(if preference { "IncludeBookDocuments=\"true\"" } else { "IncludeBookDocuments=\"false\"" }));
        let imported = designcraft_idml::import_idml(&exported).unwrap();
        assert_eq!(imported.toc.as_ref().unwrap().scope, TocScope::CurrentDocument);
        assert_eq!(imported.toc.as_ref().unwrap().idml_include_book_documents, Some(preference));
    }
}
