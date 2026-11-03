//! Placing EPS artwork: the source is kept and only the preview is rasterised.
//!
//! These helpers write real files, so they live in their own `_tests.rs` file: the
//! file-system guard in `tests/file_guard.rs` checks `src` files but exempts test ones.

use super::{Arc, AssetId, Content, ItemId, Session, base64_encode, json};

pub(crate) fn tiff_eps() -> Vec<u8> {
    let mut tiff = b"II".to_vec();
    tiff.extend(42u16.to_le_bytes());
    tiff.extend(8u32.to_le_bytes());
    tiff.extend(10u16.to_le_bytes());
    for (tag, ty, count, value) in [
        (256u16, 4u16, 1u32, 2u32),
        (257, 4, 1, 2),
        (258, 3, 3, 134),
        (259, 3, 1, 1),
        (262, 3, 1, 2),
        (273, 4, 1, 140),
        (277, 3, 1, 3),
        (278, 4, 1, 2),
        (279, 4, 1, 12),
        (284, 3, 1, 1),
    ] {
        tiff.extend(tag.to_le_bytes());
        tiff.extend(ty.to_le_bytes());
        tiff.extend(count.to_le_bytes());
        tiff.extend(value.to_le_bytes());
    }
    tiff.extend(0u32.to_le_bytes());
    for _ in 0..3 {
        tiff.extend(8u16.to_le_bytes());
    }
    tiff.extend([255, 0, 0].repeat(4));
    let ps = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 100 50\n%%EOF\n";
    let mut eps = vec![0xC5, 0xD0, 0xD3, 0xC6];
    for n in [30usize, ps.len(), 0, 0, 30 + ps.len(), tiff.len()] {
        eps.extend((n as u32).to_le_bytes());
    }
    eps.extend([255, 255]);
    eps.extend(ps);
    eps.extend(tiff);
    eps
}

#[test]
fn eps_degenerate_bounds_are_rejected_without_edits() {
    for bounds in ["0 0 1e-320 1e-320", "0 0 1e308 1e308", "0 0 inf 10"] {
        let source = format!("%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: {bounds}\n%%EOF\n");
        for command in ["file.place", "place.load"] {
            let mut s = Session::new();
            s.execute("file.new", &json!({})).unwrap();
            let before = s.doc().unwrap().doc.clone();
            let revision = s.doc().unwrap().revision;
            assert!(
                s.execute(command, &json!({"base64": base64_encode(source.as_bytes()), "name": "bad.eps", "width": 100})).is_err(),
                "{command}: {bounds}"
            );
            assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
            assert_eq!(s.doc().unwrap().revision, revision);
            assert!(s.loaded.is_none());
            designcraft_format::load(&designcraft_format::save(&s.doc().unwrap().doc).unwrap()).unwrap();
        }
    }
}

#[test]
fn eps_fitting_overflow_is_rejected_before_document_edits() {
    let source = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 0.01 0.01\n%%EOF\n";
    let params = json!({"base64": base64_encode(source), "name": "small.eps"});
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    assert!(s.execute("file.place", &json!({"base64": base64_encode(source), "name": "small.eps", "width": 1e308})).is_err());
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    s.execute("place.load", &params).unwrap();
    let before = s.doc().unwrap().doc.clone();
    assert!(s.execute("place.drop", &json!({"rect": [0, 0, 1e308, 1e308]})).is_err());
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    s.execute("place.drop", &json!({"rect": [72, 72, 172, 122]})).unwrap();
    designcraft_format::load(&designcraft_format::save(&s.doc().unwrap().doc).unwrap()).unwrap();
}

#[test]
fn eps_table_and_data_merge_reject_fitting_overflow() {
    let source = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 0.01 0.01\n%%EOF\n";
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let r = s.execute("frame.create", &json!({"rect":[72,72,372,272],"content":"text"})).unwrap();
    s.execute("text.select", &json!({"story":r["story"],"anchor":0,"focus":0})).unwrap();
    s.execute("table.insert", &json!({"rows":1,"cols":1,"width":1e308})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    assert!(s.execute("table.placeGraphic", &json!({"base64":base64_encode(source),"name":"small.eps","fit":"fill"})).is_err());
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    designcraft_format::load(&designcraft_format::save(&before).unwrap()).unwrap();

    let path = std::env::temp_dir().join(format!("dc-eps-fit-{}.eps", std::process::id()));
    std::fs::write(&path, source).unwrap();
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("data.source.select", &json!({"bytes":base64_encode(format!("@Photo\n{}\n",path.display()).as_bytes()),"name":"source.csv"})).unwrap();
    let r = s.execute("frame.create", &json!({"rect":[0,0,1e308,100],"content":"graphic"})).unwrap();
    s.execute("data.placeholder.add", &json!({"field":"Photo","item":r["id"]})).unwrap();
    let result = s.execute("data.merge", &json!({"fitting":"fillProportionally"})).unwrap();
    assert_eq!(result["missingImages"].as_array().unwrap().len(), 1);
    assert!(s.doc().unwrap().doc.assets.is_empty());
    designcraft_format::load(&designcraft_format::save(&s.doc().unwrap().doc).unwrap()).unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn eps_table_and_data_merge_use_bounding_box_points() {
    let dir = std::env::temp_dir().join(format!("dc-eps-routes-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (index, bytes) in [b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 100 50\n%%EOF\n".to_vec(), tiff_eps()].into_iter().enumerate() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72,72,372,272], "content":"text"})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor":0, "focus":0})).unwrap();
        s.execute("table.insert", &json!({"rows":1,"cols":1})).unwrap();
        s.execute("table.placeGraphic", &json!({"base64": base64_encode(&bytes), "name":"logo.eps"})).unwrap();
        let table = s.doc().unwrap().doc.stories.values().flat_map(|st| st.tables.values()).next().unwrap();
        assert_eq!(table.cells[0].graphic.as_ref().unwrap().size, (100.0, 50.0), "table {index}");
        let path = dir.join(format!("logo-{index}.eps"));
        std::fs::write(&path, &bytes).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("data.source.select", &json!({"bytes": base64_encode(format!("@Photo\n{}\n", path.display()).as_bytes()), "name":"source.csv"}))
            .unwrap();
        let r = s.execute("frame.create", &json!({"rect":[72,72,372,272],"content":"graphic"})).unwrap();
        s.execute("data.placeholder.add", &json!({"field":"Photo","item":r["id"]})).unwrap();
        let result = s.execute("data.merge", &json!({"fitting":"none"})).unwrap();
        assert!(result["missingImages"].as_array().unwrap().is_empty());
        let g = s
            .doc()
            .unwrap()
            .doc
            .spreads
            .iter()
            .flat_map(|sp| &sp.items)
            .find_map(|it| if let Content::Graphic(g) = &it.content { Some(g) } else { None })
            .unwrap();
        assert_eq!(g.size, (100.0, 50.0), "merge {index}");
        designcraft_format::load(&designcraft_format::save(&s.doc().unwrap().doc).unwrap()).unwrap();
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn eps_idml_roundtrip_preserves_source_and_resource_type() {
    use std::io::Read;
    for bytes in [b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 100 50\n%%EOF\n".to_vec(), tiff_eps()] {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.place", &json!({"base64":base64_encode(&bytes),"name":"logo.eps"})).unwrap();
        let source = s.doc().unwrap().doc.clone();
        let idml = designcraft_idml::export_idml(&source);
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(&idml)).unwrap();
        let names: Vec<_> = z.file_names().filter(|n| n.starts_with("Spreads/")).map(str::to_string).collect();
        let mut xml = String::new();
        for name in names {
            z.by_name(&name).unwrap().read_to_string(&mut xml).unwrap();
        }
        assert!(xml.contains("<EPS "), "EPS resource element missing");
        assert!(xml.contains("Encapsulated PostScript (EPS)"));
        assert!(!xml.contains("Portable Network Graphics (PNG)"));
        let restored = designcraft_idml::import_idml(&idml).unwrap();
        let a = restored.assets.values().next().unwrap();
        assert_eq!(a.mime, "application/postscript");
        assert_eq!(a.data.as_slice(), bytes);
        assert!(designcraft_render::decode_pixmap(&a.data).is_some());
        // A linked-only EPS keeps its type even when the external file is unavailable;
        // when the resolver supplies it, sniff the raw source rather than PNG metadata.
        let mut linked = (*source).clone();
        let a = Arc::make_mut(linked.assets.values_mut().next().unwrap());
        a.link = Some("/synthetic/logo.eps".into());
        let idml = designcraft_idml::export_idml_with(&linked, &designcraft_idml::ExportOptions { embed_images: false });
        let missing = designcraft_idml::import_idml_with(&idml, &|_| None).unwrap();
        let a = missing.assets.values().next().unwrap();
        assert_eq!(a.mime, "application/postscript");
        assert!(a.data.is_empty());
        let resolved = designcraft_idml::import_idml_with(&idml, &|path| {
            assert_eq!(path, "/synthetic/logo.eps");
            Some(bytes.clone())
        })
        .unwrap();
        let a = resolved.assets.values().next().unwrap();
        assert_eq!(a.mime, "application/postscript");
        assert_eq!(a.data.as_slice(), bytes);
    }
}

#[test]
fn eps_place_cursor_retains_source_and_uses_bounding_box_size() {
    let source = b"%!PS-Adobe-3.0 EPSF-3.0\n%%HiResBoundingBox: 0 0 100.5 50.25\n%%EOF\n";
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let result = s.execute("place.load", &json!({"base64": base64_encode(source), "name": "logo.eps"})).unwrap();
    let aid = AssetId(result["asset"].as_u64().unwrap());
    assert_eq!(s.doc().unwrap().doc.assets[&aid].data.as_slice(), source);
    assert_eq!(s.doc().unwrap().doc.assets[&aid].mime, "application/postscript");
    assert_eq!(s.doc().unwrap().doc.assets[&aid].pixels, Some((201, 101)));
    let result = s.execute("place.drop", &json!({"x": 72, "y": 72})).unwrap();
    let it = s.doc().unwrap().doc.item(ItemId(result["id"].as_u64().unwrap())).unwrap();
    let Content::Graphic(g) = &it.content else { panic!("expected a graphic") };
    assert_eq!(g.size, (100.5, 50.25));
    assert_eq!((it.bounds().width(), it.bounds().height()), g.size);
    let before = s.doc().unwrap().doc.clone();
    for invalid in ["%!PS-Adobe-3.0 EPSF-3.0\nno bounding box\n", "%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 inf 10\n"] {
        let corrupt = json!({"base64": base64_encode(invalid.as_bytes()), "name": "bad.eps"});
        assert!(s.execute("place.load", &corrupt).is_err());
        assert!(s.execute("file.place", &corrupt).is_err());
    }
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
}
