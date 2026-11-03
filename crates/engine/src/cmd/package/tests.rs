use super::*;

#[test]
fn relocated_packages_rebind_embedded_image_links_on_open() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("dc-package-relocation-{}-{nonce}", std::process::id()));
    let original = dir.join("original");
    let moved = dir.join("moved");
    let png = designcraft_render::Rendered { width: 4, height: 3, pixels: [30, 90, 180, 255].repeat(12) }.to_png();
    let mut s = Session::new();
    s.execute("file.new", &json!({"title": "OwnedPackage", "width": 72, "height": 72})).unwrap();
    s.execute("file.place", &json!({"base64": super::super::base64_encode(&png), "name": "owned.png", "x": 5, "y": 5, "width": 16})).unwrap();
    s.execute("file.package", &json!({"dir": original.to_string_lossy(), "idml": true, "pdf": false})).unwrap();
    std::fs::rename(&original, &moved).unwrap();
    let expected = std::path::absolute(moved.join("Links").join("owned.png")).unwrap();
    for (command, extension) in [("file.open", "designcraft"), ("file.openIdml", "idml")] {
        let mut opened = Session::new();
        opened.execute(command, &json!({"path": moved.join(format!("OwnedPackage.{extension}")).to_string_lossy()})).unwrap();
        let asset = opened.doc().unwrap().doc.assets.values().next().unwrap();
        assert_eq!(*asset.data, png, "{extension}: the packaged pixels");
        assert_eq!(asset.link.as_deref(), expected.to_str(), "{extension}: use the relocated Links file");
        let links = opened.execute("links.list", &json!({})).unwrap();
        assert_eq!(links[0]["status"], "ok", "{extension}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

/// `pdf` takes file.exportPdf's options (a PDF preset's): a PDF/X-4 package PDF; `true` is the
/// default PDF, and bad options are refused.
#[test]
fn package_pdf_takes_export_options() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("dc-package-pdf-options-{}-{nonce}", std::process::id()));
    let mut s = Session::new();
    s.execute("file.new", &json!({"title": "Options", "width": 72, "height": 72})).unwrap();
    let x4 = dir.join("x4");
    s.execute("file.package", &json!({"dir": x4.to_string_lossy(), "idml": false, "pdf": {"standard": "x4"}})).unwrap();
    let has = |path: &std::path::Path, what: &[u8]| std::fs::read(path).unwrap().windows(what.len()).any(|w| w == what);
    assert!(has(&x4.join("Options.pdf"), b"GTS_PDFX"));
    let plain = dir.join("plain");
    s.execute("file.package", &json!({"dir": plain.to_string_lossy(), "idml": false, "pdf": true})).unwrap();
    assert!(!has(&plain.join("Options.pdf"), b"GTS_PDFX"));
    // Refused before anything is written: bad options, a bad flattener or view option, and a
    // `pdf` that is neither a switch nor options.
    for bad in [
        json!({"standard": "nope"}),
        json!({"flatten": "extreme"}),
        json!({"pageLayout": "spiral"}),
        json!({"view": "zoomed"}),
        json!("print"),
        json!(3),
    ] {
        let out = dir.join("bad");
        assert!(s.execute("file.package", &json!({"dir": out.to_string_lossy(), "pdf": bad})).is_err(), "{bad}");
        assert!(!out.exists(), "{bad}: bad options write nothing");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

/// A preset that flattens transparency (Screen and Email's `flatten: low`) gives the packaged
/// PDF the flattening `file.exportPdf` does.
#[test]
fn the_packaged_pdf_is_flattened_as_export_flattens_it() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("dc-package-pdf-flatten-{}-{nonce}", std::process::id()));
    let mut s = Session::new();
    s.execute("file.new", &json!({"title": "Flat", "width": 144, "height": 144})).unwrap();
    let id = s.execute("frame.create", &json!({"rect": [10, 10, 100, 100], "content": "unassigned"})).unwrap()["id"].clone();
    s.execute("object.fill", &json!({"ids": [id], "swatch": "[Black]"})).unwrap();
    s.execute("object.opacity", &json!({"ids": [id], "opacity": 0.5})).unwrap();
    let exported = super::super::base64_decode(s.execute("file.exportPdf", &json!({"flatten": "low"})).unwrap()["base64"].as_str().unwrap());
    s.execute("file.package", &json!({"dir": dir.to_string_lossy(), "idml": false, "pdf": {"flatten": "low"}})).unwrap();
    let packaged = std::fs::read(dir.join("Flat.pdf")).unwrap();
    let images = |pdf: &[u8]| pdf.windows(b"/Subtype/Image".len()).filter(|w| *w == b"/Subtype/Image").count();
    assert!(images(&exported) > 0, "the export rasterises the transparent object");
    assert_eq!(images(&packaged), images(&exported), "so does the package");
    std::fs::remove_dir_all(dir).unwrap();
}

/// A per-placement PDF layer choice cannot travel in an IDML copy that links its files, so the
/// report says the copies differ instead of leaving it to be discovered.
#[test]
fn package_report_names_placed_pdf_layer_choices() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("dc-package-pdf-layers-{}-{nonce}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pdf = super::super::pdflayers::tests::two_layer_pdf();
    let mut s = Session::new();
    s.execute("file.new", &json!({"title": "LayeredPackage", "width": 100, "height": 100})).unwrap();
    let id = s.execute("file.place", &json!({"base64": super::super::base64_encode(&pdf), "name": "art.pdf", "x": 0, "y": 0, "width": 100})).unwrap()
        ["id"]
        .clone();
    s.execute("object.layerOptions", &json!({"ids": [id], "hidden": ["Red"]})).unwrap();
    let r = s.execute("file.package", &json!({"dir": dir.to_string_lossy(), "idml": true, "pdf": false})).unwrap();
    let report = r["report"].as_str().unwrap();
    assert!(report.contains("Placed PDF layers"), "{report}");
    // The DesignCraft copy keeps the choice; only the linked IDML copy shows file defaults.
    let packed = super::super::from_bytes(&std::fs::read(dir.join("LayeredPackage.designcraft")).unwrap()).unwrap();
    let item = packed.item(designcraft_doc::ItemId(id.as_u64().unwrap())).unwrap();
    assert_eq!(item.pdf_hidden_layers, vec!["Red".to_string()]);
    assert!(item.has_pdf_layer_override());
    std::fs::remove_dir_all(dir).unwrap();
}

/// The package's IDML links its images to the files in the package's Links folder and embeds
/// no copies of them.
#[test]
fn package_idml_links_the_links_folder_and_embeds_nothing() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("dc-package-idml-links-{}-{nonce}", std::process::id()));
    let png = designcraft_render::Rendered { width: 4, height: 3, pixels: [30, 90, 180, 255].repeat(12) }.to_png();
    let mut s = Session::new();
    s.execute("file.new", &json!({"title": "LinkedPackage", "width": 72, "height": 72})).unwrap();
    s.execute("file.place", &json!({"base64": super::super::base64_encode(&png), "name": "my photo.png", "x": 5, "y": 5, "width": 16})).unwrap();
    s.execute("file.package", &json!({"dir": dir.to_string_lossy(), "idml": true, "pdf": false})).unwrap();
    let idml = std::fs::read(dir.join("LinkedPackage.idml")).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(idml.as_slice())).unwrap();
    let mut spreads = String::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        if f.name().starts_with("Spreads/") {
            std::io::Read::read_to_string(&mut f, &mut spreads).unwrap();
        }
    }
    // Relative to the package: no path from the machine that made it (#340).
    let uri = "file:Links/my%20photo.png";
    let folder = dir.file_name().unwrap().to_string_lossy().into_owned();
    assert!(!spreads.contains(&folder), "the package's own folder is not written: {spreads}");
    assert!(spreads.contains(&format!(r#"LinkResourceURI="{uri}""#)), "{spreads}");
    assert!(!spreads.contains("<Contents") && !spreads.contains(r#"StoredState="Embedded""#), "no image is embedded");
    let back = designcraft_idml::import_idml_with(&idml, &|_| None).unwrap();
    let asset = back.assets.values().next().unwrap();
    assert!(asset.data.is_empty(), "no image is embedded");
    assert_eq!(asset.link.as_deref(), Some("Links/my photo.png"));
    assert!(dir.join("Links").join("my photo.png").exists());
}

/// A link whose file is gone has no bytes to copy: the package leaves it out of Links/, the
/// report lists it as missing, and the packaged document keeps it missing (#908).
#[test]
fn package_leaves_missing_links_missing() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("dc-package-missing-{}-{nonce}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><rect width="40" height="40"/></svg>"##;
    let (gone, kept) = (dir.join("gone.svg"), dir.join("kept.svg"));
    std::fs::write(&gone, svg).unwrap();
    std::fs::write(&kept, svg.replace("<rect", "<circle r=\"9\"/><rect")).unwrap();
    let mut s = Session::new();
    s.execute("file.new", &json!({"title": "TwoLinks"})).unwrap();
    s.execute("file.place", &json!({"path": gone.to_string_lossy(), "x": 10, "y": 10})).unwrap();
    s.execute("edit.deselectAll", &json!({})).unwrap();
    s.execute("file.place", &json!({"path": kept.to_string_lossy(), "x": 10, "y": 200})).unwrap();
    let idml = dir.join("TwoLinks.idml");
    s.execute("file.exportIdml", &json!({"path": idml.to_string_lossy(), "embedImages": false})).unwrap();
    std::fs::remove_file(&gone).unwrap();
    let mut s = Session::new();
    s.execute("file.openIdml", &json!({"path": idml.to_string_lossy()})).unwrap();
    let status = |s: &mut Session, name: &str| {
        let links = s.execute("links.list", &json!({})).unwrap();
        links.as_array().unwrap().iter().find(|l| l["name"] == name).map(|l| l["status"].clone())
    };
    assert_eq!(status(&mut s, "gone.svg"), Some(json!("missing")));

    let out = dir.join("Pkg");
    let r = s.execute("file.package", &json!({"dir": out.to_string_lossy(), "idml": true})).unwrap();
    assert!(!out.join("Links").join("gone.svg").exists(), "a missing link is not copied");
    assert!(out.join("Links").join("kept.svg").exists());
    assert!(!r["files"].to_string().contains("gone.svg"), "{}", r["files"]);
    let report = r["report"].as_str().unwrap();
    assert!(report.contains("gone.svg (missing)"), "{report}");
    for (command, extension) in [("file.open", "designcraft"), ("file.openIdml", "idml")] {
        let mut opened = Session::new();
        opened.execute(command, &json!({"path": out.join(format!("TwoLinks.{extension}")).to_string_lossy()})).unwrap();
        assert_eq!(status(&mut opened, "gone.svg"), Some(json!("missing")), "{extension}");
        assert_eq!(status(&mut opened, "kept.svg"), Some(json!("ok")), "{extension}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

/// A package holds no path from the machine that made it: the document and the IDML link the
/// copies in `Links/` by relative names, and they still open as linked after the move (#340).
#[test]
fn package_documents_hold_relative_links_only() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("dc-package-relative-{}-{nonce}", std::process::id()));
    let folder = dir.file_name().unwrap().to_string_lossy().into_owned();
    let png = designcraft_render::Rendered { width: 4, height: 3, pixels: [30, 90, 180, 255].repeat(12) }.to_png();
    let mut s = Session::new();
    s.execute("file.new", &json!({"title": "RelativePackage", "width": 72, "height": 72})).unwrap();
    s.execute("file.place", &json!({"base64": super::super::base64_encode(&png), "name": "owned.png", "x": 5, "y": 5, "width": 16})).unwrap();
    s.execute("file.package", &json!({"dir": dir.to_string_lossy(), "idml": true, "pdf": false})).unwrap();
    let packed = super::super::from_bytes(&std::fs::read(dir.join("RelativePackage.designcraft")).unwrap()).unwrap();
    let link = packed.assets.values().next().unwrap().link.clone().unwrap();
    assert_eq!(link, "Links/owned.png");
    assert!(!link.contains(&folder));
    // Copy Link(s) To is unchanged: it relinks the open document by full path.
    s.execute("links.copyTo", &json!({"dir": dir.join("Copies").to_string_lossy()})).unwrap();
    let copied = s.doc().unwrap().doc.assets.values().next().unwrap().link.clone().unwrap();
    assert!(std::path::Path::new(&copied).is_absolute(), "{copied}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn package_writes_document_links_and_report() {
    let dir = std::env::temp_dir().join(format!("dc-package-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let svg = dir.join("logo.svg");
    std::fs::write(&svg, r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><rect width="40" height="40"/></svg>"##).unwrap();
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("file.place", &json!({"path": svg.to_string_lossy(), "x": 100, "y": 100})).unwrap();
    s.execute("frame.create", &json!({"rect": [72, 300, 400, 400], "content": "text", "text": "Hello"})).unwrap();
    let out = dir.join("Pkg");
    let r = s.execute("file.package", &json!({"dir": out.to_string_lossy(), "pdf": true, "instructions": "Print on matte."})).unwrap();
    let files: Vec<String> = r["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap().to_string()).collect();
    assert!(files.iter().any(|f| f.ends_with(".designcraft")));
    assert!(files.iter().any(|f| f.ends_with(".idml")));
    assert!(files.iter().any(|f| f.ends_with(".pdf")));
    assert!(out.join("Links").join("logo.svg").exists());
    let report = std::fs::read_to_string(out.join("Instructions.txt")).unwrap();
    assert!(report.contains("Print on matte.") && report.contains("logo.svg"), "{report}");
    assert!(report.find("Print on matte.") < report.find("Package report:"), "the instructions come first: {report}");
    assert!(report.contains("Preflight:"), "{report}");
    // The packaged document links to its Links folder.
    let doc_file = files.iter().find(|f| f.ends_with(".designcraft")).unwrap();
    let packed = super::super::from_bytes(&std::fs::read(doc_file).unwrap()).unwrap();
    let link = packed.assets.values().next().unwrap().link.clone().unwrap();
    assert!(link.contains("Links"), "{link}");
    // Copy Links To relinks the open document.
    let r = s.execute("links.copyTo", &json!({"dir": dir.join("Copies").to_string_lossy()})).unwrap();
    assert_eq!(r["copied"], 1);
    assert!(s.doc().unwrap().doc.assets.values().next().unwrap().link.as_deref().unwrap().contains("Copies"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn package_copies_the_fonts_their_licences_allow() {
    use designcraft_fonts::DOCUMENT_FONTS_FOLDER;
    use designcraft_fonts::testing::{font_with, with_fs_type};
    const FREE: &str = "DocFont Package Free";
    const RESTRICTED: &str = "DocFont Package Restricted";
    let dir = std::env::temp_dir().join(format!("dc-package-fonts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let fonts = dir.join("src").join(DOCUMENT_FONTS_FOLDER);
    std::fs::create_dir_all(&fonts).unwrap();
    std::fs::write(fonts.join("free.ttf"), font_with(FREE, &['F']).unwrap()).unwrap();
    std::fs::write(fonts.join("restricted.otf"), with_fs_type(font_with(RESTRICTED, &['R']).unwrap(), 0x0002).unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(fonts.join("free.ttf"), std::fs::Permissions::from_mode(0o640)).unwrap();
    }
    // Text in both fonts, and in the bundled default font.
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Free Restricted Default"})).unwrap();
    for (a, b, family) in [(0, 4, FREE), (5, 15, RESTRICTED)] {
        s.execute("text.select", &json!({"story": r["story"], "anchor": a, "focus": b})).unwrap();
        s.execute("type.char", &json!({"fontFamily": family, "fontStyle": "Regular"})).unwrap();
    }
    let src = dir.join("src").join("Fonts.designcraft");
    s.execute("file.save", &json!({"path": src.to_string_lossy()})).unwrap();
    let mut s = Session::new();
    assert_eq!(s.execute("file.open", &json!({"path": src.to_string_lossy()})).unwrap()["documentFonts"], 2);

    let out = dir.join("Pkg");
    let r = s.execute("file.package", &json!({"dir": out.to_string_lossy(), "idml": false})).unwrap();
    let packed_fonts = out.join(DOCUMENT_FONTS_FOLDER);
    assert!(packed_fonts.join("free.ttf").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        assert_eq!(
            std::fs::metadata(packed_fonts.join("free.ttf")).unwrap().permissions().mode() & 0o777,
            std::fs::metadata(fonts.join("free.ttf")).unwrap().permissions().mode() & 0o777
        );
    }
    assert!(!packed_fonts.join("restricted.otf").exists(), "a restricted licence keeps the font out");
    assert!(r["files"].to_string().contains("free.ttf"), "{}", r["files"]);
    let report = r["report"].as_str().unwrap();
    assert!(report.contains(&format!("{RESTRICTED} Regular — not copied: its licence doesn't allow it")), "{report}");
    assert!(report.contains(&format!("{FREE} Regular\n")), "{report}");
    assert!(report.contains(&format!("{} Regular — included with DesignCraft", designcraft_fonts::DEFAULT_FAMILY)), "{report}");
    // The packaged document opens with its fonts.
    let doc = r["files"][0].as_str().unwrap().to_string();
    assert_eq!(Session::new().execute("file.open", &json!({"path": doc})).unwrap()["documentFonts"], 1);
    // Packaging into the folder the fonts come from leaves them whole.
    let len = std::fs::metadata(fonts.join("free.ttf")).unwrap().len();
    s.execute("file.package", &json!({"dir": dir.join("src").to_string_lossy(), "idml": false})).unwrap();
    assert_eq!(std::fs::metadata(fonts.join("free.ttf")).unwrap().len(), len);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn package_copies_the_fallback_fonts_that_draw_text() {
    use designcraft_fonts::DOCUMENT_FONTS_FOLDER;
    use designcraft_fonts::testing::{font_with, with_fs_type};
    const NAMED: &str = "DocFont Fallback Named";
    const HELPER: &str = "DocFont Fallback Helper";
    const RESTRICTED: &str = "DocFont Fallback Restricted";
    // Characters no other font has: the named font lacks both, the document's other fonts
    // draw them.
    let (helped, restricted) = ('\u{F0A41}', '\u{F0A42}');
    let dir = std::env::temp_dir().join(format!("dc-package-fallback-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let fonts = dir.join("src").join(DOCUMENT_FONTS_FOLDER);
    std::fs::create_dir_all(&fonts).unwrap();
    std::fs::write(fonts.join("named.ttf"), font_with(NAMED, &['N']).unwrap()).unwrap();
    std::fs::write(fonts.join("helper.ttf"), font_with(HELPER, &[helped]).unwrap()).unwrap();
    std::fs::write(fonts.join("restricted.otf"), with_fs_type(font_with(RESTRICTED, &[restricted]).unwrap(), 0x0002).unwrap()).unwrap();
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("document.preferences", &json!({"glyphFallback": true})).unwrap();
    let text = format!("N{helped}{restricted}");
    let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": text})).unwrap();
    s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": text.len()})).unwrap();
    s.execute("type.char", &json!({"fontFamily": NAMED, "fontStyle": "Regular"})).unwrap();
    let src = dir.join("src").join("Fallback.designcraft");
    // Written without File › Save's preview: drawing it before the document's fonts load would
    // pull in a system font that covers these characters (Unifont CSUR does) and draws them
    // ahead of the document's own fonts in this process.
    std::fs::write(&src, crate::cmd::to_bytes(&s.doc().unwrap().doc).unwrap()).unwrap();
    let mut s = Session::new();
    assert_eq!(s.execute("file.open", &json!({"path": src.to_string_lossy()})).unwrap()["documentFonts"], 3);

    let out = dir.join("Pkg");
    let r = s.execute("file.package", &json!({"dir": out.to_string_lossy(), "idml": false})).unwrap();
    let packed_fonts = out.join(DOCUMENT_FONTS_FOLDER);
    assert!(packed_fonts.join("named.ttf").exists());
    assert!(packed_fonts.join("helper.ttf").exists(), "the fallback font that draws text is copied");
    assert!(!packed_fonts.join("restricted.otf").exists(), "a restricted licence keeps a fallback font out too");
    let report = r["report"].as_str().unwrap();
    let fallback = report.split("Fallback fonts").nth(1).unwrap_or_else(|| panic!("{report}"));
    assert!(fallback.contains(&format!("{HELPER} Regular\n")), "{report}");
    assert!(fallback.contains(&format!("{RESTRICTED} Regular — not copied: its licence doesn't allow it")), "{report}");
    assert!(!fallback.contains(NAMED), "{report}");
    // The packaged document draws the same text with its fonts.
    let doc = r["files"][0].as_str().unwrap().to_string();
    assert_eq!(Session::new().execute("file.open", &json!({"path": doc})).unwrap()["documentFonts"], 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn packaged_fonts_keep_a_font_extension() {
    use designcraft_fonts::DOCUMENT_FONTS_FOLDER;
    use designcraft_fonts::testing::{font_with, installed_face};
    // A font service's file without an extension is copied under a name the reopened
    // package loads.
    let dir = std::env::temp_dir().join(format!("dc-package-font-ext-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let from = dir.join(".12345");
    let bytes = font_with("DocFont Package Extensionless", &['E']).unwrap();
    std::fs::write(&from, &bytes).unwrap();
    let face = installed_face(bytes, from).unwrap();
    let root = OutputRoot::new(dir.clone()).unwrap();
    let fonts = dir.join("Pkg").join(DOCUMENT_FONTS_FOLDER);
    std::fs::create_dir_all(&fonts).unwrap();
    let (mut copied, mut taken, mut files) = (Vec::new(), Vec::new(), Vec::new());
    assert_eq!(copy_font(&root, &face, &fonts, &mut copied, &mut taken, &mut files), "");
    assert_eq!(files, [fonts.join("12345.ttf")]);
    let db = designcraft_fonts::FontDb::global();
    let loaded = db.load_document_fonts(&fonts, &|_| Ok(()));
    assert_eq!((loaded.faces, loaded.skipped.len()), (1, 0), "{:?}", loaded.skipped);
    db.close_scope(loaded.scope);
    // The first four bytes name the kind; a font extension or an unknown kind stays.
    assert_eq!(font_file_name("Face".into(), b"OTTO...."), "Face.otf");
    assert_eq!(font_file_name("Faces".into(), b"ttcf...."), "Faces.ttc");
    assert_eq!(font_file_name("Mac".into(), b"true...."), "Mac.ttf");
    assert_eq!(font_file_name("..".into(), &[0, 1, 0, 0]), "font.ttf");
    assert_eq!(font_file_name("Face.OTF".into(), b"ttcf"), "Face.OTF");
    assert_eq!(font_file_name("Face.dfont".into(), b"\0\0\x01\0"), "Face.dfont");
    assert_eq!(font_file_name("short".into(), b"OT"), "short");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn package_uses_portable_names_inside_the_selected_directory() {
    assert_eq!(safe_component("Archive/Annual: Review", "Untitled"), "Annual_ Review");
    assert_eq!(safe_component(".", "Untitled"), "Untitled");
    assert_eq!(safe_component("CON", "Untitled"), "_CON");
    assert_eq!(safe_component("COM¹.txt", "Untitled"), "_COM¹.txt");
    assert_eq!(safe_component("LPT³", "Untitled"), "_LPT³");
    for name in ["COM¹", "COM²", "COM³", "LPT¹", "LPT²", "LPT³"] {
        assert!(is_reserved_windows_name(name), "{name}");
    }
    let mut taken = Vec::new();
    assert_eq!(unique_name("Logo.svg".into(), &mut taken), "Logo.svg");
    assert_eq!(unique_name("logo.svg".into(), &mut taken), "logo 2.svg");
    assert_eq!(unique_name("Résumé.pdf".into(), &mut taken), "Résumé.pdf");
    assert_eq!(unique_name("Re\u{301}sume\u{301}.pdf".into(), &mut taken), "Re\u{301}sume\u{301} 2.pdf");
    assert_eq!(unique_name("ΟΣ.txt".into(), &mut taken), "ΟΣ.txt");
    assert_eq!(unique_name("ος.txt".into(), &mut taken), "ος 2.txt");

    let dir = std::env::temp_dir().join(format!("dc-package-safe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = Session::new();
    s.execute("file.new", &json!({"title": "Archive/Annual: Review"})).unwrap();
    let r = s.execute("file.package", &json!({"dir": dir.to_string_lossy(), "idml": false})).unwrap();
    let files = r["files"].as_array().unwrap();
    assert!(files.iter().all(|path| Path::new(path.as_str().unwrap()).starts_with(&dir)));
    assert!(dir.join("Annual_ Review.designcraft").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn package_overwrites_existing_regular_outputs() {
    let dir = std::env::temp_dir().join(format!("dc-package-overwrite-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let document = dir.join("Annual.designcraft");
    std::fs::write(&document, b"previous package").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(&document, std::fs::Permissions::from_mode(0o640)).unwrap();
    }

    let mut s = Session::new();
    s.execute("file.new", &json!({"title": "Annual"})).unwrap();
    s.execute("file.package", &json!({"dir": dir.to_string_lossy(), "idml": false})).unwrap();
    s.execute("file.package", &json!({"dir": dir.to_string_lossy(), "idml": false})).unwrap();
    assert!(super::super::from_bytes(&std::fs::read(&document).unwrap()).is_ok());
    assert!(!dir.join(designcraft_fonts::DOCUMENT_FONTS_FOLDER).exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        assert_eq!(std::fs::metadata(&document).unwrap().permissions().mode() & 0o777, 0o640);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn package_accepts_a_relative_destination() {
    let dir = PathBuf::from(format!(".dc-package-relative-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = Session::new();
    s.execute("file.new", &json!({"title": "Relative"})).unwrap();
    s.execute("file.package", &json!({"dir": dir.to_string_lossy(), "idml": false})).unwrap();
    assert!(dir.join("Relative.designcraft").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn package_refuses_redirected_destinations() {
    use std::os::unix::fs::symlink;

    let base = std::env::temp_dir().join(format!("dc-package-redirects-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let outside = base.join("separate");
    std::fs::create_dir_all(&outside).unwrap();
    let mut s = Session::new();
    s.execute("file.new", &json!({"title": "Annual"})).unwrap();

    let redirected_root = base.join("redirected-package");
    symlink(&outside, &redirected_root).unwrap();
    assert!(s.execute("file.package", &json!({"dir": redirected_root.to_string_lossy(), "idml": false})).is_err());
    assert!(!outside.join("Annual.designcraft").exists());
    std::fs::remove_file(&redirected_root).unwrap();

    // A linked folder above the package is followed, as `/tmp` is on macOS.
    let linked_parent = base.join("linked-parent");
    symlink(&outside, &linked_parent).unwrap();
    let nested_package = linked_parent.join("nested").join("Package");
    s.execute("file.package", &json!({"dir": nested_package.to_string_lossy(), "idml": false})).unwrap();
    assert!(outside.join("nested").join("Package").join("Annual.designcraft").is_file());
    std::fs::remove_file(&linked_parent).unwrap();

    let package = base.join("Package");
    std::fs::create_dir_all(&package).unwrap();
    symlink(&outside, package.join("Links")).unwrap();
    assert!(s.execute("file.package", &json!({"dir": package.to_string_lossy(), "idml": false})).is_err());
    assert!(!outside.join("Annual.designcraft").exists());
    std::fs::remove_file(package.join("Links")).unwrap();
    std::fs::create_dir(package.join("Links")).unwrap();

    let kept = outside.join("kept.designcraft");
    std::fs::write(&kept, b"kept").unwrap();
    symlink(&kept, package.join("Annual.designcraft")).unwrap();
    assert!(s.execute("file.package", &json!({"dir": package.to_string_lossy(), "idml": false})).is_err());
    assert_eq!(std::fs::read(&kept).unwrap(), b"kept");
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(windows)]
#[test]
fn package_refuses_windows_directory_redirects() {
    use std::os::windows::fs::symlink_dir;

    let base = std::env::temp_dir().join(format!("dc-package-windows-redirect-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let outside = base.join("separate");
    std::fs::create_dir_all(&outside).unwrap();
    let redirected = base.join("Package");
    // A symbolic link needs Developer Mode or elevation (os error 1314 without them); a
    // directory junction needs neither, and `is_redirect` treats both as redirects.
    if symlink_dir(&outside, &redirected).is_err() && !make_junction(&outside, &redirected) {
        eprintln!("skipped: directory redirects can't be made here");
        let _ = std::fs::remove_dir_all(&base);
        return;
    }
    let mut s = Session::new();
    s.execute("file.new", &json!({"title": "Annual"})).unwrap();
    assert!(s.execute("file.package", &json!({"dir": redirected.to_string_lossy(), "idml": false})).is_err());
    assert!(!outside.join("Annual.designcraft").exists());
    std::fs::remove_dir(&redirected).unwrap();
    let _ = std::fs::remove_dir_all(&base);
}

/// Makes `link` a directory junction to `target` with `mklink /J`, and reports whether it was made.
#[cfg(windows)]
fn make_junction(target: &Path, link: &Path) -> bool {
    use std::os::windows::process::CommandExt;

    // Windows paths can't hold `"`, so quoting each one keeps `&` and spaces in it literal for cmd.
    std::process::Command::new("cmd")
        .args(["/D", "/C"])
        .raw_arg(format!("mklink /J \"{}\" \"{}\"", link.display(), target.display()))
        .output()
        .is_ok_and(|output| output.status.success())
}
