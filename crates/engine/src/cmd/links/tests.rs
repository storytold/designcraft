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

/// A file's time `secs` ago (a file changed in the last two seconds is always read).
#[cfg(not(target_arch = "wasm32"))]
fn set_age(path: &Path, secs: u64) {
    let f = std::fs::File::options().write(true).open(path).unwrap();
    f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(secs)).unwrap();
}

/// The size and time an image was loaded at are kept with it, so a file that still has them is
/// not read at all, also on the first check after a document is opened (nothing is remembered
/// yet then, and reading every link of a document on a network share takes minutes). The bytes
/// here differ from the file's on purpose: a status that read the file would say modified.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_linked_image_at_its_loaded_size_and_time_is_not_read() {
    let dir = std::env::temp_dir().join(format!("dc-image-link-loaded-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("photo.bin");
    std::fs::write(&path, b"file").unwrap();
    set_age(&path, 60);
    let loaded = crate::cmd::datamerge::fingerprint_of(&path).unwrap();
    let asset = |link_fingerprint| designcraft_doc::Asset {
        id: AssetId(1),
        link: Some(path.to_string_lossy().into_owned()),
        link_fingerprint,
        data: designcraft_doc::AssetData::embedded(b"data".to_vec()),
        ..Default::default()
    };
    let status = |a: &designcraft_doc::Asset| status_cached(a, None, &mut ImageChecks::default());

    assert_eq!(status(&asset(Some(loaded))), "ok", "the size and time it was loaded at: not read");
    // No fingerprint (a document from before they were kept) or another time: compared.
    assert_eq!(status(&asset(None)), "modified");
    assert_eq!(status(&asset(Some(Fingerprint { mtime: loaded.mtime - 60, ..loaded }))), "modified", "a new time alone is compared");
    // Bytes replaced without their fingerprint: it is of another size, so it isn't theirs.
    let swapped = designcraft_doc::Asset { data: designcraft_doc::AssetData::embedded(b"other bytes".to_vec()), ..asset(Some(loaded)) };
    assert_eq!(status(&swapped), "modified");
    // A time too recent to trust: another write in the same second would keep it.
    set_age(&path, 0);
    let recent = crate::cmd::datamerge::fingerprint_of(&path).unwrap();
    assert_eq!(status(&asset(Some(recent))), "modified");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file saved again unchanged, or copied, restored from a backup or checked out again, has a
/// new time and the same bytes: it stays OK (only a new size is conclusive), read once per new
/// time. The fingerprint is saved with the document, so every link of a copied project folder
/// would otherwise show Modified.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_graphic_rewritten_with_the_same_bytes_stays_ok() {
    let dir = std::env::temp_dir().join(format!("dc-link-touched-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("logo.png");
    let image = png(2, 2);
    let rewrite = |bytes: &[u8], age: u64| {
        std::fs::write(&path, bytes).unwrap();
        set_age(&path, age);
    };
    rewrite(&image, 300);
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let aid = AssetId(s.execute("file.place", &json!({"path": path.to_string_lossy()})).unwrap()["asset"].as_u64().unwrap());
    rewrite(&image, 60);
    let a = s.doc().unwrap().doc.assets[&aid].clone();
    assert!(a.link_fingerprint.is_some());
    assert_ne!(a.link_fingerprint, crate::cmd::datamerge::fingerprint_of(&path), "the file has another time than when it was placed");
    let mut checks = ImageChecks::default();
    assert_eq!(status_cached(&a, None, &mut checks), "ok");
    assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");

    // One byte changed, same size, at a time not seen before: compared again.
    let mut changed = image.clone();
    if let Some(last) = changed.last_mut() {
        *last ^= 0xff;
    }
    rewrite(&changed, 120);
    assert_eq!(status_cached(&a, None, &mut checks), "modified");
    assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "modified");
    // Update remembers the file it read, so the next check needs no read.
    assert_eq!(s.execute("links.update", &json!({})).unwrap()["updated"], 1);
    let a = s.doc().unwrap().doc.assets[&aid].clone();
    assert_eq!(a.link_fingerprint, crate::cmd::datamerge::fingerprint_of(&path));
    assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The rows a UI shows while the files are checked on a worker thread come from the document
/// alone: no file is looked at, not even for its size, so a share that doesn't answer can't
/// hold them back.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn preview_rows_look_at_no_file() {
    let dir = std::env::temp_dir().join(format!("dc-link-preview-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (linked, embedded, gone) = (dir.join("linked.png"), dir.join("embedded.png"), dir.join("gone.png"));
    for (path, width) in [(&linked, 10), (&embedded, 11), (&gone, 12)] {
        std::fs::write(path, png(width, 10)).unwrap();
        set_age(path, 60);
    }
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let place = |s: &mut Session, path: &Path, x: u32| {
        AssetId(s.execute("file.place", &json!({"path": path.to_string_lossy(), "x": x, "y": 72})).unwrap()["asset"].as_u64().unwrap())
    };
    let ids = [place(&mut s, &linked, 72), place(&mut s, &embedded, 144), place(&mut s, &gone, 216)];
    s.execute("links.embed", &json!({"asset": ids[1].0})).unwrap();
    std::fs::remove_file(&gone).unwrap();
    let st = s.doc().unwrap();
    assert!(st.doc.assets[&ids[0]].link_fingerprint.is_some(), "a placed file is remembered by its size and time");
    assert!(st.doc.assets[&ids[1]].link_fingerprint.is_none(), "embed forgets the file");
    let statuses = |rows: &[Value]| -> Vec<String> {
        ids.iter().map(|id| rows.iter().find(|r| r["asset"] == id.0).unwrap()["status"].as_str().unwrap().to_string()).collect()
    };

    let (preview, seen) = crate::io::touched(|| preview_document_links(&st.doc, &s.cache, None));
    assert!(seen.is_empty(), "the preview looked at {seen:?}");
    assert_eq!(statuses(&preview), ["checking", "embedded", "checking"]);

    // The check fills them in, with the rest of each row as in the preview.
    let checked = list_document_links(&st.doc, &s.cache, None, &mut LinkListChecks::default(), &AtomicBool::new(false)).unwrap();
    assert_eq!(statuses(&checked), ["ok", "embedded", "missing"]);
    let without_status = |rows: &[Value]| -> Vec<Value> {
        rows.iter()
            .cloned()
            .map(|mut row| {
                if let Some(row) = row.as_object_mut() {
                    row.remove("status");
                }
                row
            })
            .collect()
    };
    assert_eq!(without_status(&preview), without_status(&checked));
    assert_eq!(Value::Array(checked), s.execute("links.list", &json!({})).unwrap(), "the same rows as links.list");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The size and time kept with bytes read from a linked file vouch for them later only if the
/// file wasn't being written then: a file changed in the same second, or still being synced,
/// keeps its time and size, and a status trusting them would say OK for other bytes.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_file_written_just_now_is_not_vouched_for_by_its_time() {
    let dir = std::env::temp_dir().join(format!("dc-link-settled-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("photo.png");
    let image = png(3, 3);
    std::fs::write(&path, &image).unwrap();
    let now = crate::cmd::datamerge::fingerprint_of(&path).unwrap();
    assert_eq!(loaded_fingerprint(Some(now), &image), None, "written in the last two seconds");
    set_age(&path, 60);
    let settled = crate::cmd::datamerge::fingerprint_of(&path).unwrap();
    assert_eq!(loaded_fingerprint(Some(settled), &image), Some(settled));
    assert_eq!(loaded_fingerprint(Some(settled), &image[1..]), None, "the file changed while it was read");
    assert_eq!(loaded_fingerprint(None, &image), None);

    // Placed while it is that recent, then replaced by other bytes of the same size at the same
    // time: the status reads the file instead of trusting the time.
    std::fs::write(&path, &image).unwrap();
    let written = std::fs::metadata(&path).unwrap().modified().unwrap();
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let aid = AssetId(s.execute("file.place", &json!({"path": path.to_string_lossy()})).unwrap()["asset"].as_u64().unwrap());
    assert_eq!(s.doc().unwrap().doc.assets[&aid].link_fingerprint, None);
    let mut other = image.clone();
    if let Some(last) = other.last_mut() {
        *last ^= 0xff;
    }
    std::fs::write(&path, &other).unwrap();
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(written).unwrap();
    assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "modified");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A check on a worker thread is told to stop when its document is no longer the one shown: it
/// ends before the next file instead of asking a share that doesn't answer about hundreds more.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_stopped_check_looks_at_no_further_file() {
    let dir = std::env::temp_dir().join(format!("dc-link-stop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    for (name, x) in [("a.png", 72), ("b.png", 144)] {
        let path = dir.join(name);
        std::fs::write(&path, png(x, 10)).unwrap();
        s.execute("file.place", &json!({"path": path.to_string_lossy(), "x": x, "y": 72})).unwrap();
    }
    let st = s.doc().unwrap();
    let mut checks = LinkListChecks::default();

    let stop = AtomicBool::new(true);
    let (rows, seen) = crate::io::touched(|| list_document_links(&st.doc, &s.cache, None, &mut checks, &stop));
    assert_eq!(rows, None);
    assert!(seen.is_empty(), "a stopped check looked at {seen:?}");

    stop.store(false, Ordering::Relaxed);
    assert_eq!(list_document_links(&st.doc, &s.cache, None, &mut checks, &stop).map(|rows| rows.len()), Some(2));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A copied or restored project folder gives every linked file a new time. The check then
/// compares the bytes, and what it found is kept with the document, so neither later checks nor
/// the next session read the file again. The document isn't marked as changed by that.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_file_found_unchanged_by_its_bytes_is_remembered_with_the_document() {
    let dir = std::env::temp_dir().join(format!("dc-link-remember-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("photo.png");
    std::fs::write(&path, png(4, 4)).unwrap();
    set_age(&path, 300);
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let aid = AssetId(s.execute("file.place", &json!({"path": path.to_string_lossy()})).unwrap()["asset"].as_u64().unwrap());
    s.execute("file.saveAs", &json!({"path": dir.join("doc.designcraft").to_string_lossy()})).unwrap();
    assert!(!s.doc().unwrap().is_dirty());
    set_age(&path, 60);
    let file = crate::cmd::datamerge::fingerprint_of(&path).unwrap();
    let check = |s: &Session, checks: &mut LinkListChecks| {
        let st = s.doc().unwrap();
        let rows = list_document_links(&st.doc, &s.cache, st.path.as_deref().map(Path::new), checks, &AtomicBool::new(false)).unwrap();
        (rows[0]["status"].as_str().unwrap().to_string(), checks.take_verified())
    };

    // Read once and found unchanged; found again from what the check remembers.
    let mut checks = LinkListChecks::default();
    assert_eq!(check(&s, &mut checks), ("ok".to_string(), vec![(aid, file)]));
    assert_eq!(checks.len(), 1);
    assert_eq!(check(&s, &mut checks), ("ok".to_string(), vec![(aid, file)]));

    // Not for another document state, another size, or a graphic that isn't linked.
    let checked = s.doc().unwrap().doc.clone();
    let other = Arc::new((*checked).clone());
    assert_eq!(remember_link_files(&mut s, &other, &[(aid, file)]), 0);
    assert_eq!(remember_link_files(&mut s, &checked, &[(aid, Fingerprint { size: file.size + 1, ..file })]), 0);
    assert_eq!(remember_link_files(&mut s, &checked, &[(AssetId(u64::MAX), file)]), 0);
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &checked), "nothing kept, nothing changed");

    assert_eq!(remember_link_files(&mut s, &checked, &[(aid, file)]), 1);
    let st = s.doc().unwrap();
    assert_eq!(st.doc.assets[&aid].link_fingerprint, Some(file));
    assert!(!st.is_dirty(), "a saved document stays saved");
    let kept = s.doc().unwrap().doc.clone();
    assert_eq!(remember_link_files(&mut s, &kept, &[(aid, file)]), 0, "already kept");
    // The next check, with nothing remembered (another session), reads nothing and has nothing new.
    assert_eq!(check(&s, &mut LinkListChecks::default()), ("ok".to_string(), vec![]));

    // What one worker remembers goes to the next.
    let mut pooled = LinkListChecks::default();
    assert!(pooled.is_empty());
    pooled.absorb(checks);
    assert_eq!(pooled.len(), 1);
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
        Arc::make_mut(d.assets.get_mut(&aid).unwrap()).data = designcraft_doc::AssetData::embedded(b"%!PS-Adobe-3.0 EPSF-3.0\n%%EOF\n".to_vec());
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
    assert_eq!(reopened.assets[&aid].data.load().unwrap().as_slice(), first);
    let mut s = Session::new();
    s.add_document(crate::DocState::new(reopened, None));
    assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");
    std::fs::write(&path, changed).unwrap();
    assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "modified");
    assert_eq!(s.execute("links.update", &json!({})).unwrap()["updated"], 1);
    assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");
    assert_eq!(s.doc().unwrap().doc.assets[&aid].data.load().unwrap().as_slice(), changed);
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
    assert_eq!(status_cached(&embedded.assets[&aid], None, &mut Default::default()), "embedded");
    assert_eq!(embedded.assets[&aid].data.load().unwrap().as_slice(), changed);
    assert!(designcraft_render::decode_pixmap(&embedded.assets[&aid].data.load().unwrap()).is_some());
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
        a.data = designcraft_doc::AssetData::embedded(proxy);
        a.mime = "image/png".into();
        Ok(())
    })
    .unwrap();
    let old = designcraft_format::load(&designcraft_format::save(&s.doc().unwrap().doc).unwrap()).unwrap();
    let mut s = Session::new();
    s.add_document(crate::DocState::new(old, None));
    assert_eq!(s.execute("links.update", &json!({})).unwrap()["updated"], 1);
    assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");
    assert_eq!(s.doc().unwrap().doc.assets[&aid].data.load().unwrap().as_slice(), source);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(designcraft_render::decode_pixmap(&s.doc().unwrap().doc.assets[&aid].data.load().unwrap()).is_some(), "old preview remains renderable");
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
        a.data = designcraft_doc::AssetData::embedded(source.to_vec());
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
    assert_eq!(before.assets[&aid].data.load().unwrap().as_slice(), source);
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
    assert_eq!(a.data.load().unwrap().as_slice(), eps(30).as_bytes());
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
    assert!(designcraft_render::render_pdf_page(&a.data.load().unwrap(), a.page as usize, 64).is_some());
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
        assert!(designcraft_render::render_pdf_page(&st.doc.assets[&aid].data.load().unwrap(), 1, 64).is_some());
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
