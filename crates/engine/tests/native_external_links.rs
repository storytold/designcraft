#![cfg(not(target_arch = "wasm32"))]

use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use designcraft_engine::Session;
use serde_json::{Value, json};

fn png(seed: u8) -> Vec<u8> {
    let pixels = (0..256 * 256 * 4).map(|n| if n % 4 == 3 { 255 } else { (n * 73 + n / 101 + usize::from(seed)) as u8 }).collect();
    designcraft_render::Rendered { width: 256, height: 256, pixels }.to_png()
}

fn setup(root: &Path) -> (Session, std::path::PathBuf, Vec<u8>) {
    let source = root.join("Links/art.png");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    let bytes = png(1);
    std::fs::write(&source, &bytes).unwrap();
    let mut session = Session::new();
    session.execute("file.new", &json!({"width": 300, "height": 300, "facingPages": false})).unwrap();
    session.execute("file.place", &json!({"path": source, "x": 0, "y": 0, "width": 200})).unwrap();
    (session, source, bytes)
}

fn archive(path: &Path) -> (Vec<String>, Value, u64) {
    let bytes = std::fs::read(path).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let names = zip.file_names().map(str::to_owned).collect();
    let document = serde_json::from_reader(zip.by_name("document.json").unwrap()).unwrap();
    let meta: Value = serde_json::from_reader(zip.by_name("meta.json").unwrap()).unwrap();
    (names, document, meta["version"].as_u64().unwrap())
}

fn assets(path: &Path) -> usize {
    archive(path).0.iter().filter(|name| name.starts_with("assets/")).count()
}

#[test]
fn native_working_links_obey_automation_read_roots() {
    use designcraft_engine::file_access::{AutomationRoots, confine};
    let temp = tempfile::tempdir().unwrap();
    let inside = temp.path().join("allowed");
    std::fs::create_dir_all(&inside).unwrap();
    let (mut session, _, _) = setup(&temp.path().join("outside"));
    let path = inside.join("linked.designcraft");
    session.execute("file.saveAs", &json!({"path": path, "embedImages": false})).unwrap();
    let original = std::fs::read(&path).unwrap();
    let roots = AutomationRoots::new(Some(&inside), Some(&inside)).unwrap().unwrap();
    confine(Some(&roots), || {
        let mut opened = Session::new();
        let result = opened.execute("file.open", &json!({"path": path})).unwrap();
        assert!(result["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("outside the read root")));
        assert!(opened.doc().unwrap().doc.assets.values().all(|a| a.data.is_empty()), "document paths cannot hydrate outside the allowed root");
        assert!(session.execute("file.save", &json!({})).is_err(), "a cached outside source cannot be silently omitted");
    });
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[test]
fn linked_save_is_small_reopens_and_exports_the_same_pdf() {
    let temp = tempfile::tempdir().unwrap();
    let (mut session, _, original) = setup(temp.path());
    let embedded = temp.path().join("embedded.designcraft");
    session.execute("file.saveAs", &json!({"path": embedded})).unwrap();
    let pdf_options = designcraft_pdf::PdfOptions { created: Some(0), ..Default::default() };
    let before_pdf_bytes = designcraft_pdf::export_pdf(&session.doc().unwrap().doc, &session.cache, &pdf_options).unwrap();
    let before_pdf = temp.path().join("before.pdf");
    session.execute("file.exportPdf", &json!({"path": before_pdf})).unwrap();
    let linked = temp.path().join("linked.designcraft");
    session.execute("file.saveAs", &json!({"path": linked, "embedImages": false})).unwrap();
    assert_eq!(assets(&embedded), 1);
    assert_eq!(assets(&linked), 0);
    assert!(std::fs::metadata(&linked).unwrap().len() < std::fs::metadata(&embedded).unwrap().len());
    assert_eq!(archive(&embedded).2, 2, "ordinary documents still open in older readers");
    assert_eq!(archive(&linked).2, 4, "older readers must reject external native artwork");
    let mut reopened = Session::new();
    let opened = reopened.execute("file.open", &json!({"path": linked})).unwrap();
    assert_eq!(opened["warnings"], json!([]));
    assert_eq!(reopened.execute("links.storage", &json!({})).unwrap()["mode"], "linked");
    assert_eq!(reopened.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");
    assert_eq!(reopened.doc().unwrap().doc.assets.values().next().unwrap().data.as_slice(), original);
    let after_pdf = temp.path().join("after.pdf");
    reopened.execute("file.exportPdf", &json!({"path": after_pdf})).unwrap();
    let after_pdf_bytes = designcraft_pdf::export_pdf(&reopened.doc().unwrap().doc, &reopened.cache, &pdf_options).unwrap();
    assert_eq!(before_pdf_bytes, after_pdf_bytes, "fixed creation dates make byte equality independent of wall-clock timing");
    assert!(std::fs::read(before_pdf).unwrap().starts_with(b"%PDF"));
    assert!(std::fs::read(after_pdf).unwrap().starts_with(b"%PDF"));
    reopened.execute("file.save", &json!({})).unwrap();
    assert_eq!(assets(&linked), 0, "the next Save remembers linked storage");
}

#[test]
fn relocating_the_folder_uses_relative_links_even_while_originals_still_exist() {
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("original");
    let (mut session, _, bytes) = setup(&original);
    let file = original.join("job.designcraft");
    session.execute("file.saveAs", &json!({"path": file, "embedImages": false})).unwrap();
    let saved = designcraft_format::load(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(saved.assets.values().next().unwrap().link.as_deref(), Some("Links/art.png"));
    let moved = temp.path().join("moved");
    std::fs::create_dir_all(moved.join("Links")).unwrap();
    std::fs::copy(&file, moved.join("job.designcraft")).unwrap();
    std::fs::write(moved.join("Links/art.png"), &bytes).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.open", &json!({"path": moved.join("job.designcraft")})).unwrap();
    let asset = reopened.doc().unwrap().doc.assets.values().next().unwrap();
    assert_eq!(asset.link.as_deref(), moved.join("Links/art.png").to_str());
    assert_eq!(asset.data.as_slice(), bytes);
}

#[test]
fn save_copy_rebases_without_changing_live_paths_or_saved_state() {
    let temp = tempfile::tempdir().unwrap();
    let (mut session, _, bytes) = setup(temp.path());
    let path = temp.path().join("job.designcraft");
    session.execute("file.saveAs", &json!({"path": path, "embedImages": false})).unwrap();
    let before = session.doc().unwrap().clone();
    let copy = temp.path().join("copies/job.designcraft");
    std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
    session.execute("file.saveACopy", &json!({"path": copy})).unwrap();
    let after = session.doc().unwrap();
    assert!(Arc::ptr_eq(&before.doc, &after.doc));
    assert!(Arc::ptr_eq(&before.saved_doc, &after.saved_doc));
    assert_eq!(before.path, after.path);
    assert_eq!(before.revision, after.revision);
    let mut reopened = Session::new();
    reopened.execute("file.open", &json!({"path": copy})).unwrap();
    assert_eq!(reopened.doc().unwrap().doc.assets.values().next().unwrap().data.as_slice(), bytes);
    assert_eq!(assets(&copy), 0);
}

#[test]
fn missing_original_does_not_destroy_the_cached_copy_or_saved_file() {
    let temp = tempfile::tempdir().unwrap();
    let (mut session, source, _) = setup(temp.path());
    let path = temp.path().join("job.designcraft");
    session.execute("file.saveAs", &json!({"path": path})).unwrap();
    let previous = std::fs::read(&path).unwrap();
    let before = session.doc().unwrap().clone();
    std::fs::remove_file(source).unwrap();
    assert!(session.execute("file.save", &json!({"embedImages": false})).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), previous);
    assert!(Arc::ptr_eq(&before.doc, &session.doc().unwrap().doc));
    assert!(Arc::ptr_eq(&before.saved_doc, &session.doc().unwrap().saved_doc));
    assert_eq!(before.path, session.doc().unwrap().path);
    assert_eq!(before.revision, session.doc().unwrap().revision);
    assert!(session.doc().unwrap().doc.assets.values().all(|asset| !asset.data.is_empty()));
}

#[test]
fn unresolved_links_warn_can_be_relinked_and_cannot_be_silently_embedded() {
    let temp = tempfile::tempdir().unwrap();
    let (mut session, source, bytes) = setup(temp.path());
    let path = temp.path().join("job.designcraft");
    session.execute("file.saveAs", &json!({"path": path, "embedImages": false})).unwrap();
    std::fs::remove_file(&source).unwrap();
    let mut reopened = Session::new();
    let opened = reopened.execute("file.open", &json!({"path": path})).unwrap();
    assert_eq!(opened["warnings"].as_array().unwrap().len(), 1);
    let links = reopened.execute("links.list", &json!({})).unwrap();
    assert_eq!(links[0]["status"], "missing");
    let id = &links[0]["asset"];
    assert!(reopened.execute("links.embed", &json!({"asset": id})).is_err());
    assert!(reopened.execute("links.storage", &json!({"mode": "embedded"})).is_err());
    assert!(reopened.execute("file.save", &json!({"embedImages": true})).is_err());
    std::fs::write(&source, &bytes).unwrap();
    reopened.execute("links.relink", &json!({"asset": id, "path": source})).unwrap();
    assert_eq!(reopened.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");
    reopened.execute("links.embed", &json!({"asset": id})).unwrap();
    reopened.execute("file.save", &json!({})).unwrap();
    assert_eq!(assets(&path), 1, "explicitly embedded artwork is kept in linked mode too");
}

#[test]
fn portable_serialization_keeps_originals_and_open_bytes_never_reads_external_paths() {
    let temp = tempfile::tempdir().unwrap();
    let (mut session, _, bytes) = setup(temp.path());
    let path = temp.path().join("job.designcraft");
    session.execute("file.saveAs", &json!({"path": path, "embedImages": false})).unwrap();
    let portable = session.execute("file.serialize", &json!({})).unwrap();
    let mut recovered = Session::new();
    recovered.execute("file.openBytes", &json!({"base64": portable["base64"]})).unwrap();
    assert_eq!(recovered.doc().unwrap().doc.assets.values().next().unwrap().data.as_slice(), bytes);
    let mut no_context = Session::new();
    let opened =
        no_context.execute("file.openBytes", &json!({"base64": designcraft_engine::cmd::base64_encode(&std::fs::read(&path).unwrap())})).unwrap();
    assert_eq!(opened["warnings"].as_array().unwrap().len(), 1);
    assert!(no_context.doc().unwrap().doc.assets.values().next().unwrap().data.is_empty());
}

#[test]
fn storage_choice_is_undoable_and_rejects_invalid_input() {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    assert!(session.execute("links.storage", &json!({"mode": "other"})).is_err());
    session.execute("links.storage", &json!({"mode": "linked"})).unwrap();
    assert!(session.doc().unwrap().doc.settings.external_links);
    session.execute("edit.undo", &json!({})).unwrap();
    assert!(!session.doc().unwrap().doc.settings.external_links);
    session.execute("edit.redo", &json!({})).unwrap();
    assert!(session.doc().unwrap().doc.settings.external_links);
}

#[test]
fn package_collects_originals_without_reembedding_the_native_working_file() {
    let temp = tempfile::tempdir().unwrap();
    let (mut session, _, bytes) = setup(temp.path());
    session.execute("links.storage", &json!({"mode": "linked"})).unwrap();
    let package = temp.path().join("package");
    let result = session.execute("file.package", &json!({"dir": package, "idml": false})).unwrap();
    let document = result["files"].as_array().unwrap().iter().filter_map(Value::as_str).find(|p| p.ends_with(".designcraft")).unwrap();
    assert_eq!(assets(Path::new(document)), 0);
    assert_eq!(std::fs::read(package.join("Links/art.png")).unwrap(), bytes);
    let mut reopened = Session::new();
    reopened.execute("file.open", &json!({"path": document})).unwrap();
    assert_eq!(reopened.doc().unwrap().doc.assets.values().next().unwrap().data.as_slice(), bytes);
}

#[test]
fn invalid_or_oversized_external_sources_do_not_get_loaded_as_artwork() {
    let temp = tempfile::tempdir().unwrap();
    let (mut session, source, _) = setup(temp.path());
    let path = temp.path().join("job.designcraft");
    session.execute("file.saveAs", &json!({"path": path, "embedImages": false})).unwrap();
    std::fs::write(&source, "this is not the linked image").unwrap();
    let mut reopened = Session::new();
    let report = reopened.execute("file.open", &json!({"path": path})).unwrap();
    assert_eq!(report["warnings"].as_array().unwrap().len(), 1);
    assert!(reopened.doc().unwrap().doc.assets.values().next().unwrap().data.is_empty());
    let oversized = std::fs::File::create(&source).unwrap();
    oversized.set_len(512 * 1024 * 1024 + 1).unwrap();
    let report = reopened.execute("file.open", &json!({"path": path})).unwrap();
    assert!(report["warnings"][0].as_str().unwrap().contains("512 MiB"));
    assert!(session.execute("file.save", &json!({})).is_err());
}

#[test]
fn reopening_uses_current_source_dimensions_for_effective_resolution() {
    let temp = tempfile::tempdir().unwrap();
    let (mut session, source, _) = setup(temp.path());
    let path = temp.path().join("job.designcraft");
    session.execute("file.saveAs", &json!({"path": path, "embedImages": false})).unwrap();
    let replacement = designcraft_render::Rendered { width: 512, height: 256, pixels: vec![128; 512 * 256 * 4] }.to_png();
    std::fs::write(source, &replacement).unwrap();
    let mut reopened = Session::new();
    let result = reopened.execute("file.open", &json!({"path": path})).unwrap();
    assert_eq!(result["warnings"], json!([]));
    let links = reopened.execute("links.list", &json!({})).unwrap();
    assert_eq!(links[0]["pixels"], json!([512, 256]));
    assert_eq!(reopened.doc().unwrap().doc.assets.values().next().unwrap().data.as_slice(), replacement);
}

#[test]
fn choosing_a_baseline_explicitly_adopts_current_metrics_and_undo_restores_legacy() {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let result = session.execute("frame.create", &json!({"rect": [10, 10, 200, 200], "content": "text"})).unwrap();
    let id = designcraft_doc::ItemId(result["id"].as_u64().unwrap());
    session
        .edit(|doc, _| {
            doc.item_mut(id).unwrap().text_frame_mut().unwrap().options.legacy_first_baseline = true;
            Ok(Value::Null)
        })
        .unwrap();
    session.execute("object.textFrameOptions", &json!({"ids": [id.0], "inset": 2, "firstBaseline": "ascent"})).unwrap();
    assert!(
        session.doc().unwrap().doc.item(id).unwrap().text_frame().unwrap().options.legacy_first_baseline,
        "unrelated frame edits preserve layout compatibility"
    );
    session.execute("object.textFrameOptions", &json!({"ids": [id.0], "firstBaseline": "capHeight"})).unwrap();
    assert!(!session.doc().unwrap().doc.item(id).unwrap().text_frame().unwrap().options.legacy_first_baseline);
    session.execute("edit.undo", &json!({})).unwrap();
    assert!(session.doc().unwrap().doc.item(id).unwrap().text_frame().unwrap().options.legacy_first_baseline);
}

#[test]
fn linked_save_keeps_recent_preview_and_normalized_extension() {
    let temp = tempfile::tempdir().unwrap();
    let (mut session, _, _) = setup(temp.path());
    let requested = temp.path().join("linked-working");
    let written = temp.path().join("linked-working.designcraft");
    let result = session.execute("file.saveAs", &json!({"path": requested, "embedImages": false})).unwrap();
    assert_eq!(result["path"], written.to_str().unwrap());
    assert_eq!(assets(&written), 0);
    let preview = designcraft_format::read_preview(std::fs::File::open(&written).unwrap()).unwrap();
    assert!(preview.starts_with(b"\x89PNG"), "linked files retain the upstream recent-file preview");
    let copy = session.execute("file.saveACopy", &json!({"path": temp.path().join("copy")})).unwrap();
    let copy = Path::new(copy["path"].as_str().unwrap());
    assert_eq!(assets(copy), 0);
    assert!(designcraft_format::read_preview(std::fs::File::open(copy).unwrap()).is_some());
    let mut reopened = Session::new();
    reopened.execute("file.open", &json!({"path": copy})).unwrap();
    assert_eq!(reopened.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");
}

#[test]
fn changed_external_pdf_recalculates_its_saved_crop() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.pdf");
    let mut origin = Session::new();
    origin.execute("file.new", &json!({})).unwrap();
    origin.execute("layout.documentSetup", &json!({"bleed": 9})).unwrap();
    origin.execute("file.exportPdf", &json!({"path": source, "bleed": true})).unwrap();
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    session.execute("file.place", &json!({"path": source, "pdfCrop": "trim", "width": 315})).unwrap();
    let before = session.doc().unwrap().doc.assets.values().next().unwrap().pdf_box.unwrap();
    let path = temp.path().join("layout.designcraft");
    session.execute("file.saveAs", &json!({"path": path, "embedImages": false})).unwrap();
    origin.execute("layout.documentSetup", &json!({"bleed": 18})).unwrap();
    origin.execute("file.exportPdf", &json!({"path": source, "bleed": true})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.open", &json!({"path": path})).unwrap();
    let asset = reopened.doc().unwrap().doc.assets.values().next().unwrap();
    let expected = designcraft_render::pdf_crop_box(&asset.data, asset.page as usize, asset.pdf_crop).unwrap().0;
    assert_ne!(before, expected);
    assert_eq!(asset.pdf_box, Some(expected), "the cached crop belongs to the new source bytes");
}

#[test]
fn external_eps_retains_originals_on_reopen_and_migrates_legacy_preview_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("logo.eps");
    let bytes = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 20 10\n%%EOF\n";
    std::fs::write(&source, bytes).unwrap();
    for legacy_preview in [false, true] {
        let mut session = Session::new();
        session.execute("file.new", &json!({})).unwrap();
        session.execute("file.place", &json!({"path": source})).unwrap();
        let aid = *session.doc().unwrap().doc.assets.keys().next().unwrap();
        if legacy_preview {
            session
                .edit(|d, _| {
                    let asset = Arc::make_mut(d.assets.get_mut(&aid).unwrap());
                    asset.data = Arc::new(designcraft_images::eps_proxy(bytes).unwrap().0);
                    asset.mime = "image/png".into();
                    Ok(())
                })
                .unwrap();
        }
        let path = temp.path().join(format!("linked-{legacy_preview}.designcraft"));
        session.execute("file.saveAs", &json!({"path": path, "embedImages": false})).unwrap();
        assert_eq!(archive(&path).2, 4);
        assert_eq!(assets(&path), 0);
        let mut reopened = Session::new();
        let result = reopened.execute("file.open", &json!({"path": path})).unwrap();
        assert_eq!(result["warnings"], json!([]));
        let asset = &reopened.doc().unwrap().doc.assets[&aid];
        assert_eq!(asset.data.as_slice(), bytes);
        assert_eq!(asset.mime, "application/postscript");
        assert!(designcraft_render::decode_pixmap(&asset.data).is_some());
        assert_eq!(reopened.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");
        reopened.execute("file.save", &json!({"embedImages": true})).unwrap();
        let embedded = designcraft_format::load(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(embedded.assets[&aid].data.as_slice(), bytes);
        assert_eq!(archive(&path).2, 3, "embedded EPS uses upstream's reader capability");
    }
}
