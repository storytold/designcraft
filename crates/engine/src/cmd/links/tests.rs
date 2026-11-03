use super::*;

fn png(w: u32, h: u32) -> Vec<u8> {
    designcraft_render::Rendered { width: w, height: h, pixels: vec![200u8; (w * h * 4) as usize] }.to_png()
}

/// The Links panel polls `links.list` twice a second: a linked image whose file keeps its size
/// and time is not read again (press-size images would otherwise be read whole on every poll).
/// A new time is read once; changed bytes show as modified; an update is remembered.
#[cfg(all(unix, not(target_arch = "wasm32")))]
#[test]
fn unchanged_linked_images_are_not_read_again() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("dc-image-link-poll-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("photo.png");
    // Two valid images of the same byte length (a uniform fill compresses to the same size
    // whatever its value), so a change shows only in the bytes.
    let a = png(40, 40);
    let b = designcraft_render::Rendered { width: 40, height: 40, pixels: vec![100u8; 40 * 40 * 4] }.to_png();
    assert_eq!(a.len(), b.len(), "the test needs two files of one size");
    assert_ne!(a, b);
    std::fs::write(&path, &a).unwrap();
    // Unreadable: a status that reads the file would say missing.
    let readable = |on: bool| std::fs::set_permissions(&path, std::fs::Permissions::from_mode(if on { 0o644 } else { 0o000 })).unwrap();
    // A time `secs` ago (a file changed in the last two seconds is always read).
    let touch = |secs: u64| {
        let f = std::fs::File::options().write(true).open(&path).unwrap();
        f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(secs)).unwrap();
    };
    touch(60);
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("file.place", &json!({"path": path.to_string_lossy(), "link": true})).unwrap();
    let status = |s: &mut Session| {
        let l = s.execute("links.list", &json!({})).unwrap();
        l.as_array().unwrap().iter().find(|r| r.get("asset").is_some()).unwrap()["status"].as_str().unwrap().to_string()
    };

    // First poll reads and remembers; the second doesn't read.
    assert_eq!(status(&mut s), "ok");
    readable(false);
    if std::fs::read(&path).is_ok() {
        // Running as root: permissions don't stop reads, so this can't be observed.
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    assert_eq!(status(&mut s), "ok", "same size and time: not read");
    // Saved again unchanged (a new time): read once, then not again.
    readable(true);
    touch(30);
    assert_eq!(status(&mut s), "ok");
    readable(false);
    assert_eq!(status(&mut s), "ok", "the second poll doesn't read it");
    // Changed, same size, new time: modified — and remembered, not re-read.
    readable(true);
    std::fs::write(&path, &b).unwrap();
    touch(20);
    assert_eq!(status(&mut s), "modified");
    readable(false);
    assert_eq!(status(&mut s), "modified", "a modified answer is remembered too");
    // Updated: the document's bytes are the file's, so the next poll reads once and says ok.
    readable(true);
    assert_eq!(s.execute("links.update", &json!({})).unwrap()["updated"], 1);
    assert_eq!(status(&mut s), "ok");
    readable(false);
    assert_eq!(status(&mut s), "ok", "remembered after the update");
    // A different size needs no read at all.
    readable(true);
    std::fs::write(&path, png(41, 41)).unwrap();
    touch(10);
    readable(false);
    assert_eq!(status(&mut s), "modified", "size differs: decided from metadata");
    readable(true);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A linked file's size is the document's choice, so a link is read under a cap (#1596).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_linked_file_over_the_cap_is_refused_instead_of_read() {
    let dir = std::env::temp_dir().join(format!("dc-link-cap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("big.png");
    let bytes = png(40, 40);
    std::fs::write(&path, &bytes).unwrap();

    // Under the cap the file is read and measured as before.
    let (read, px) = reload_capped(&path, bytes.len() as u64).expect("a file within the cap is read");
    assert_eq!((read.len(), px), (bytes.len(), (40, 40)));

    // Over it the file is refused by name, and the error says what the limit is.
    let err = reload_capped(&path, (bytes.len() - 1) as u64).expect_err("a file over the cap is refused");
    let msg = err.to_string();
    assert!(msg.contains("big.png") && msg.contains("larger than"), "the error names the file and the limit: {msg}");

    // The real cap is the one packaging uses, so an ordinary graphic still loads.
    assert!(reload(&path).is_ok(), "a normal linked graphic is unaffected");
    let _ = std::fs::remove_dir_all(&dir);
}

fn pdf(sizes: &[(u32, u32)]) -> Vec<u8> {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": sizes.len(), "facingPages": false})).unwrap();
    for (i, (width, height)) in sizes.iter().enumerate() {
        s.execute("layout.pageSize", &json!({"pages": [i + 1], "width": width, "height": height})).unwrap();
    }
    let result = s.execute("file.exportPdf", &json!({})).unwrap();
    super::super::file::base64_decode(result["base64"].as_str().unwrap())
}

#[test]
fn anchored_image_lists_its_page_and_resolution() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 2, "facingPages": false})).unwrap();
    let r = s
        .execute("frame.create", &json!({"spread": 1, "rect": [72, 72, 400, 400], "content": "text", "text": "Before after and more words."}))
        .unwrap();
    let sid = s.doc().unwrap().doc.item(ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
    let placed = s
        .execute(
            "file.place",
            &json!({"base64": super::super::file::base64_encode(&png(10, 10)), "name": "dot.png", "x": 450, "y": 450, "width": 36}),
        )
        .unwrap();
    let id = placed["id"].as_u64().unwrap();
    let before = s.execute("links.list", &json!({})).unwrap();
    assert_eq!(before[0]["uses"][0]["ppi"], 20.0);
    s.execute("anchored.insert", &json!({"ids": [id], "story": sid.0, "pos": 7})).unwrap();
    let list = s.execute("links.list", &json!({})).unwrap();
    let uses = list[0]["uses"].as_array().unwrap();
    assert_eq!(uses.len(), 1, "anchored image must have one use: {list}");
    assert_eq!(uses[0]["page"], "2");
    assert_eq!(uses[0]["ppi"], 20.0);
}

#[test]
fn eps_web_exports_convert_tiff_and_skip_bad_used_assets() {
    use std::io::Read;
    let bytes = super::super::file::eps_place_tests::tiff_eps();
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let placed = s.execute("file.place", &json!({"base64":super::super::file::base64_encode(&bytes),"name":"logo.eps"})).unwrap();
    let aid = AssetId(placed["asset"].as_u64().unwrap());
    let before = s.doc().unwrap().doc.clone();
    let html = s.execute("file.exportHtml", &json!({})).unwrap();
    let encoded = html["text"].as_str().unwrap().split("data:image/png;base64,").nth(1).unwrap().split('"').next().unwrap();
    let png = super::super::file::base64_decode(encoded);
    assert!(png.starts_with(b"\x89PNG"));
    assert_eq!(designcraft_render::image_size(&png), Some((2, 2)));
    assert_eq!(designcraft_render::decode_pixmap(&png).unwrap().data()[0], designcraft_render::decode_pixmap(&bytes).unwrap().data()[0]);
    let result = s.execute("file.exportEpub", &json!({})).unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(super::super::file::base64_decode(result["base64"].as_str().unwrap()))).unwrap();
    let mut image = Vec::new();
    z.by_name(&format!("OEBPS/images/logo-{}.png", aid.0)).unwrap().read_to_end(&mut image).unwrap();
    assert_eq!(image, png);
    let mut opf = String::new();
    z.by_name("OEBPS/content.opf").unwrap().read_to_string(&mut opf).unwrap();
    assert!(opf.contains("media-type=\"image/png\""));
    assert!(!opf.contains("image/tiff"));
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    s.edit(|d, _| {
        Arc::make_mut(d.assets.get_mut(&aid).unwrap()).data = Arc::new(b"%!PS-Adobe-3.0 EPSF-3.0\n%%EOF\n".to_vec());
        Ok(())
    })
    .unwrap();
    let before = s.doc().unwrap().doc.clone();
    for command in ["file.exportHtml", "file.exportEpub"] {
        let result = s.execute(command, &json!({})).unwrap();
        assert_eq!(result["warnings"].as_array().unwrap().len(), 1, "{command}");
        if command == "file.exportHtml" {
            assert!(!result["text"].as_str().unwrap().contains("<img "));
        }
    }
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    s.execute("edit.clear", &json!({})).unwrap();
    assert!(s.doc().unwrap().doc.assets.contains_key(&aid));
    for command in ["file.exportHtml", "file.exportEpub"] {
        let result = s.execute(command, &json!({})).unwrap();
        assert!(result["warnings"].as_array().unwrap().is_empty(), "unused EPS: {command}");
    }
}

#[test]
fn eps_status_tracks_source_bytes_across_save_update_relink_and_embed() {
    let dir = std::env::temp_dir().join(format!("dc-eps-status-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("logo.eps");
    let first = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 20 10\n0 0 0 setrgbcolor\n%%EOF\n";
    let changed = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 20 10\n1 0 0 setrgbcolor\n%%EOF\n";
    assert_eq!(first.len(), changed.len());
    assert_eq!(designcraft_images::eps_proxy(first), designcraft_images::eps_proxy(changed), "same preview, different source");
    std::fs::write(&path, first).unwrap();
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let aid = AssetId(s.execute("file.place", &json!({"path": path.to_string_lossy()})).unwrap()["asset"].as_u64().unwrap());
    assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");
    let before = s.doc().unwrap().doc.clone();
    assert_eq!(s.execute("links.update", &json!({})).unwrap()["updated"], 0);
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before), "unchanged EPS must not create an edit");
    let saved = designcraft_format::save(&before).unwrap();
    let reopened = designcraft_format::load(&saved).unwrap();
    assert_eq!(reopened.assets[&aid].data.as_slice(), first);
    let mut s = Session::new();
    s.add_document(crate::DocState::new(reopened, None));
    assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");
    std::fs::write(&path, changed).unwrap();
    assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "modified");
    assert_eq!(s.execute("links.update", &json!({})).unwrap()["updated"], 1);
    assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");
    assert_eq!(s.doc().unwrap().doc.assets[&aid].data.as_slice(), changed);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "missing");
    let replacement = dir.join("replacement.eps");
    std::fs::write(&replacement, changed).unwrap();
    s.execute("links.relink", &json!({"asset": aid.0, "path": replacement.to_string_lossy()})).unwrap();
    assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "missing");
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");
    s.execute("links.embed", &json!({"asset": aid.0})).unwrap();
    assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "embedded");
    std::fs::remove_file(&replacement).unwrap();
    let embedded = designcraft_format::load(&designcraft_format::save(&s.doc().unwrap().doc).unwrap()).unwrap();
    // The asset is embedded, so no link is followed and the document is irrelevant.
    assert_eq!(status(&embedded.assets[&aid], None), "embedded");
    assert_eq!(embedded.assets[&aid].data.as_slice(), changed);
    assert!(designcraft_render::decode_pixmap(&embedded.assets[&aid].data).is_some());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn eps_legacy_preview_document_is_upgraded_by_update() {
    let dir = std::env::temp_dir().join(format!("dc-eps-legacy-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("logo.eps");
    let source = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 20 10\n%%EOF\n";
    std::fs::write(&path, source).unwrap();
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let aid = AssetId(s.execute("file.place", &json!({"path": path.to_string_lossy()})).unwrap()["asset"].as_u64().unwrap());
    let proxy = designcraft_images::eps_proxy(source).unwrap().0;
    s.edit(|d, _| {
        let a = Arc::make_mut(d.assets.get_mut(&aid).unwrap());
        a.data = Arc::new(proxy);
        a.mime = "image/png".into();
        Ok(())
    })
    .unwrap();
    let old = designcraft_format::load(&designcraft_format::save(&s.doc().unwrap().doc).unwrap()).unwrap();
    let mut s = Session::new();
    s.add_document(crate::DocState::new(old, None));
    assert_eq!(s.execute("links.update", &json!({})).unwrap()["updated"], 1);
    assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");
    assert_eq!(s.doc().unwrap().doc.assets[&aid].data.as_slice(), source);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(designcraft_render::decode_pixmap(&s.doc().unwrap().doc.assets[&aid].data).is_some(), "old preview remains renderable");
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn eps_source_exports_preview_to_pdf_html_and_epub_without_editing_document() {
    use hayro_syntax::object::Name;
    use std::io::Read;

    let source = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 20 10\n%%EOF\n";
    let proxy = designcraft_images::eps_proxy(source).unwrap().0;
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let aid = AssetId(
        s.execute("file.place", &json!({"base64": super::super::file::base64_encode(source), "name": "logo.eps"})).unwrap()["asset"]
            .as_u64()
            .unwrap(),
    );
    // A document retaining the original EPS rather than the legacy preview-only asset.
    s.edit(|d, _| {
        let a = Arc::make_mut(d.assets.get_mut(&aid).unwrap());
        a.data = Arc::new(source.to_vec());
        a.mime = "application/postscript".into();
        Ok(())
    })
    .unwrap();
    let before = s.doc().unwrap().doc.clone();
    let result = s.execute("file.exportPdf", &json!({})).unwrap();
    assert!(result["warnings"].as_array().unwrap().is_empty(), "{:?}", result["warnings"]);
    let pdf = hayro_syntax::Pdf::new(super::super::file::base64_decode(result["base64"].as_str().unwrap())).unwrap();
    assert_eq!(
        pdf.objects()
            .into_iter()
            .filter_map(|o| o.into_stream())
            .filter(|st| st.dict().get::<Name>("Subtype").is_some_and(|n| n.as_str() == "Image"))
            .count(),
        1
    );
    let result = s.execute("file.exportHtml", &json!({})).unwrap();
    let encoded = result["text"].as_str().unwrap().split("data:image/png;base64,").nth(1).unwrap().split('"').next().unwrap();
    assert_eq!(super::super::file::base64_decode(encoded), proxy);
    let result = s.execute("file.exportEpub", &json!({})).unwrap();
    let bytes = super::super::file::base64_decode(result["base64"].as_str().unwrap());
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut image = Vec::new();
    archive.by_name(&format!("OEBPS/images/logo-{}.png", aid.0)).unwrap().read_to_end(&mut image).unwrap();
    assert_eq!(image, proxy);
    let mut opf = String::new();
    archive.by_name("OEBPS/content.opf").unwrap().read_to_string(&mut opf).unwrap();
    assert!(opf.contains("media-type=\"image/png\""));
    assert!(!opf.contains("application/postscript"));
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before), "exports must leave the source document intact");
    assert_eq!(before.assets[&aid].data.as_slice(), source);
}

#[test]
fn eps_relink_update_and_folder_relink_retain_source() {
    let dir = std::env::temp_dir().join(format!("dc-links-eps-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("new")).unwrap();
    let eps = |w: u32| format!("%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 {w} 10\n%%EOF\n");
    let original = dir.join("original.eps");
    let replacement = dir.join("replacement.eps");
    std::fs::write(&original, eps(10)).unwrap();
    std::fs::write(&replacement, eps(30)).unwrap();
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("file.place", &json!({"path": original.to_string_lossy()})).unwrap();
    let aid = s.doc().unwrap().doc.assets.keys().next().copied().unwrap();
    std::fs::write(&original, eps(20)).unwrap();
    assert_eq!(s.execute("links.update", &json!({})).unwrap()["updated"], 1);
    assert_eq!(s.doc().unwrap().doc.assets[&aid].pixels, Some((40, 20)));
    s.execute("links.relink", &json!({"asset": aid.0, "path": replacement.to_string_lossy()})).unwrap();
    let a = &s.doc().unwrap().doc.assets[&aid];
    assert_eq!(a.pixels, Some((60, 20)));
    assert_eq!(a.mime, "application/postscript");
    assert_eq!(a.data.as_slice(), eps(30).as_bytes());
    let moved = dir.join("new").join("replacement.eps");
    std::fs::rename(&replacement, &moved).unwrap();
    assert_eq!(s.execute("links.relinkFolder", &json!({"dir": dir.join("new").to_string_lossy()})).unwrap()["relinked"], 1);
    assert_eq!(s.doc().unwrap().doc.assets[&aid].link.as_deref(), moved.to_str());
    assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn pdf_relink_and_update_preserve_selected_page_size() {
    let dir = std::env::temp_dir().join(format!("dc-links-pdf-size-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let original = dir.join("original.pdf");
    let replacement = dir.join("replacement.pdf");
    std::fs::write(&original, pdf(&[(10, 10), (20, 30)])).unwrap();
    std::fs::write(&replacement, pdf(&[(40, 40), (50, 60)])).unwrap();
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("file.place", &json!({"path": original.to_string_lossy(), "pdfPage": 2})).unwrap();
    let aid = s.doc().unwrap().doc.assets.keys().next().copied().unwrap();
    s.execute("links.relink", &json!({"asset": aid.0, "path": replacement.to_string_lossy()})).unwrap();
    let a = &s.doc().unwrap().doc.assets[&aid];
    assert_eq!(a.page, 1);
    assert_eq!(a.pixels, Some((50, 60)));
    assert!(designcraft_render::render_pdf_page(&a.data, a.page as usize, 64).is_some());
    std::fs::write(&replacement, pdf(&[(70, 70), (80, 90)])).unwrap();
    assert_eq!(s.execute("links.update", &json!({})).unwrap()["updated"], 1);
    assert_eq!(s.doc().unwrap().doc.assets[&aid].pixels, Some((80, 90)));
    let raster = dir.join("replacement.png");
    std::fs::write(&raster, png(4, 5)).unwrap();
    s.execute("links.relink", &json!({"asset": aid.0, "path": raster.to_string_lossy()})).unwrap();
    assert_eq!(s.doc().unwrap().doc.assets[&aid].page, 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn pdf_reload_rejects_missing_page_without_edit() {
    let full = pdf(&[(10, 10), (20, 30)]);
    let short = pdf(&[(10, 10)]);
    let dir = std::env::temp_dir().join(format!("dc-links-pdf-page-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("new")).unwrap();
    let original = dir.join("book.pdf");
    let replacement = dir.join("new/book.pdf");
    std::fs::write(&replacement, short.clone()).unwrap();
    for command in ["links.relink", "links.relinkFolder", "links.update"] {
        std::fs::write(&original, &full).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.place", &json!({"path": original.to_string_lossy(), "pdfPage": 2})).unwrap();
        let aid = s.doc().unwrap().doc.assets.keys().next().copied().unwrap();
        if command == "links.update" {
            std::fs::write(&original, &short).unwrap();
        }
        let st = s.doc().unwrap();
        let before = st.doc.clone();
        let revision = st.revision;
        let undo = st.history.undo.len();
        let journal = s.journal.len();
        let params = match command {
            "links.relinkFolder" => json!({"dir": dir.join("new").to_string_lossy(), "missingOnly": false}),
            "links.update" => json!({"asset": aid.0}),
            _ => json!({"asset": aid.0, "path": replacement.to_string_lossy()}),
        };
        let err = s.execute(command, &params).unwrap_err();
        assert!(err.to_string().contains("PDF page 2"), "{command}: {err}");
        let st = s.doc().unwrap();
        assert!(Arc::ptr_eq(&st.doc, &before), "{command}");
        assert_eq!(st.revision, revision);
        assert_eq!(st.history.undo.len(), undo);
        assert_eq!(s.journal.len(), journal);
        assert!(designcraft_render::render_pdf_page(&st.doc.assets[&aid].data, 1, 64).is_some());
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn link_status_relink_update_embed() {
    let dir = std::env::temp_dir().join(format!("dc-links-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("a.png");
    let b = dir.join("b.png");
    std::fs::write(&a, png(10, 10)).unwrap();
    std::fs::write(&b, png(20, 10)).unwrap();
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("file.place", &json!({"path": a.to_string_lossy(), "x": 72, "y": 72, "width": 72})).unwrap();
    let l = s.execute("links.list", &json!({})).unwrap();
    assert_eq!(l[0]["status"], "ok");
    assert_eq!(l[0]["uses"][0]["ppi"], 10.0, "10 px over 1 inch");
    let aid = l[0]["asset"].as_u64().unwrap();
    // The file changes on disk.
    std::fs::write(&a, png(30, 10)).unwrap();
    assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "modified");
    s.execute("links.update", &json!({})).unwrap();
    let l = s.execute("links.list", &json!({})).unwrap();
    assert_eq!(l[0]["status"], "ok");
    assert_eq!(l[0]["pixels"], json!([30, 10]));
    // Missing, then relinked.
    std::fs::remove_file(&a).unwrap();
    assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "missing");
    s.execute("links.relink", &json!({"asset": aid, "path": b.to_string_lossy()})).unwrap();
    let l = s.execute("links.list", &json!({})).unwrap();
    assert_eq!((l[0]["status"].as_str(), l[0]["name"].as_str()), (Some("ok"), Some("b.png")));
    // Go to selects the frame; embed forgets the file.
    let r = s.execute("links.goTo", &json!({"asset": aid})).unwrap();
    assert_eq!(r["ids"].as_array().unwrap().len(), 1);
    s.execute("links.embed", &json!({"asset": aid})).unwrap();
    assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "embedded");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Sets a file's modification time `secs` seconds ago (a time old enough to be trusted).
fn age(path: &std::path::Path, secs: u64) {
    let t = std::time::SystemTime::now() - std::time::Duration::from_secs(secs);
    std::fs::File::options().write(true).open(path).unwrap().set_modified(t).unwrap();
}

/// Files whose contents a link check read on this thread while `f` ran.
fn reads<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let before = check::READS.with(std::cell::Cell::get);
    let out = f();
    (out, check::READS.with(std::cell::Cell::get) - before)
}

fn first_row(s: &mut Session, p: Value) -> Value {
    s.execute("links.list", &p).unwrap()[0].clone()
}

/// Asking again doesn't read the file: placing it remembered its size and time; a file touched
/// without a change is read once and stays OK; a size change is seen from the metadata alone.
#[test]
fn link_status_reads_a_file_only_when_its_size_and_time_cannot_tell() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.png");
    std::fs::write(&file, png(6, 6)).unwrap();
    age(&file, 60);
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("file.place", &json!({"path": file.to_string_lossy()})).unwrap();
    // Unchanged: many polls, no reads.
    let (rows, n) = reads(|| (0..10).map(|_| first_row(&mut s, json!({}))["status"].clone()).collect::<Vec<_>>());
    assert!(rows.iter().all(|r| r == "ok"), "{rows:?}");
    assert_eq!(n, 0, "an unchanged file is never read");
    // Touched, same contents: the metadata alone can't tell, so it is read once, then trusted.
    age(&file, 30);
    let (r, n) = reads(|| first_row(&mut s, json!({"metadataOnly": true})));
    assert_eq!((r["status"].clone(), r["unverified"].clone(), n), (json!("ok"), json!(true), 0));
    let ((a, b), n) = reads(|| (first_row(&mut s, json!({}))["status"].clone(), first_row(&mut s, json!({}))["status"].clone()));
    assert_eq!((a, b, n), (json!("ok"), json!("ok"), 1));
    let (r, n) = reads(|| first_row(&mut s, json!({"metadataOnly": true})));
    assert_eq!((r["status"].clone(), r.get("unverified").cloned(), n), (json!("ok"), None, 0));
    // A different size: modified, from the metadata.
    std::fs::write(&file, png(7, 6)).unwrap();
    let (r, n) = reads(|| first_row(&mut s, json!({"metadataOnly": true})));
    assert_eq!((r["status"].clone(), n), (json!("modified"), 0));
    let (r, n) = reads(|| first_row(&mut s, json!({})));
    assert_eq!((r["status"].clone(), n), (json!("modified"), 0));
    // Missing.
    std::fs::remove_file(&file).unwrap();
    assert_eq!(first_row(&mut s, json!({"metadataOnly": true}))["status"], "missing");
}

/// The same size with other bytes and another time: a metadata-only look can't tell (it reads
/// as OK, unverified), a full one reads the file once and says modified, and Update reloads it.
/// Undo brings back the old copy, which is compared again rather than trusted.
#[test]
fn a_same_size_change_is_found_by_one_read_and_undo_compares_again() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("swatch.png");
    let solid = |rgba: [u8; 4]| designcraft_render::Rendered { width: 4, height: 4, pixels: rgba.repeat(16) }.to_png();
    let (red, blue) = (solid([255, 0, 0, 255]), solid([0, 0, 255, 255]));
    assert_eq!(red.len(), blue.len(), "the same size, so only the bytes tell them apart");
    std::fs::write(&file, &red).unwrap();
    age(&file, 60);
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let aid = s.execute("file.place", &json!({"path": file.to_string_lossy()})).unwrap()["asset"].clone();
    std::fs::write(&file, &blue).unwrap();
    age(&file, 30);
    let (r, n) = reads(|| first_row(&mut s, json!({"metadataOnly": true})));
    assert_eq!((r["status"].clone(), r["unverified"].clone(), n), (json!("ok"), json!(true), 0));
    let (r, n) = reads(|| first_row(&mut s, json!({})));
    assert_eq!((r["status"].clone(), n), (json!("modified"), 1));
    // Remembered: the next looks don't read it.
    let (r, n) = reads(|| first_row(&mut s, json!({"metadataOnly": true})));
    assert_eq!((r["status"].clone(), n), (json!("modified"), 0));
    let undo = s.doc().unwrap().history.undo.len();
    assert_eq!(s.execute("links.update", &json!({"assets": [aid]})).unwrap()["updated"], 1);
    assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1, "one undo step");
    let (r, n) = reads(|| first_row(&mut s, json!({})));
    assert_eq!((r["status"].clone(), n), (json!("ok"), 0), "the update remembered what it read");
    assert_eq!(s.doc().unwrap().doc.assets[&AssetId(aid.as_u64().unwrap())].data.as_slice(), blue.as_slice());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(first_row(&mut s, json!({}))["status"], "modified");
}

/// A link to something that isn't a regular file is missing, never read; an empty copy is
/// missing; an unlinked graphic is embedded; `links.update {assets}` refuses what isn't a list
/// of ids.
#[cfg(unix)]
#[test]
fn hostile_links_are_missing_without_being_read() {
    let dir = tempfile::tempdir().unwrap();
    let data = Arc::new(png(2, 2));
    for link in ["/dev/zero", "/dev/null", "relative.png", "", "\0", "file:///dev/zero", "//server/share/x.png", "/no/such/dir/x.png"] {
        let (state, n) = reads(|| link_state(Some(link), None, &data, true));
        assert_eq!((state, n), (LinkState::Missing, 0), "{link:?}");
    }
    let folder = dir.path().to_string_lossy().to_string();
    assert_eq!(link_state(Some(&folder), None, &data, true), LinkState::Missing);
    let file = dir.path().join("x.png");
    std::fs::write(&file, png(2, 2)).unwrap();
    assert_eq!(link_state(Some(&file.to_string_lossy()), None, &Arc::new(Vec::new()), true), LinkState::Missing);
    assert_eq!(link_state(None, None, &data, true), LinkState::Embedded);
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    for bad in [json!({"assets": 3}), json!({"assets": ["x"]}), json!({"assets": [-1]}), json!({"assets": [1.5]})] {
        assert!(s.execute("links.update", &bad).is_err(), "{bad}");
    }
    assert_eq!(s.execute("links.update", &json!({"assets": [u64::MAX]})).unwrap()["updated"], 0);
}
