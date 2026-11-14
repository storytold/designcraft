//! Export, preflight, package and alpha wrap with linked images: synthetic files in a temp dir.
#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use designcraft_doc::{AssetData, AssetId, LinkedSource, Proxy, Storage};
use serde_json::{Value, json};

use crate::Session;
use crate::asset_io::{content_hash, fingerprint_from};

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("dc-linked-output-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A `w` × `h` PNG; transparent in its left half when `cut`.
fn png(w: usize, h: usize, cut: bool) -> Vec<u8> {
    let mut pixels = Vec::new();
    for _ in 0..h {
        for x in 0..w {
            pixels.extend([200, 40, 40, if cut && x < w / 2 { 0 } else { 255 }]);
        }
    }
    designcraft_render::Rendered { width: w as u32, height: h as u32, pixels }.to_png()
}

/// A session with `bytes` placed, then made a linked asset to `file` (holding `bytes`) with a
/// proxy of them → the asset.
fn linked_session(file: &Path, bytes: &[u8]) -> (Session, AssetId) {
    std::fs::write(file, bytes).unwrap();
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("file.place", &json!({"base64": crate::cmd::base64_encode(bytes), "name": "art.png", "x": 10, "y": 10, "width": 100})).unwrap();
    let id = *s.doc().unwrap().doc.assets.keys().next().unwrap();
    let proxy = designcraft_render::proxy::make_proxy(bytes, 0).unwrap();
    let st = s.doc_mut().unwrap();
    let a = Arc::make_mut(Arc::make_mut(&mut st.doc).assets.get_mut(&id).unwrap());
    a.link = Some(file.to_string_lossy().into_owned());
    a.link_fingerprint = Some(fingerprint_from(&std::fs::metadata(file).unwrap()));
    a.link_hash = Some(content_hash(bytes));
    a.storage = Storage::Linked;
    a.data = AssetData::linked(LinkedSource::of(a, None).unwrap());
    a.proxy = Some(Proxy { mime: proxy.mime.into(), pixels: proxy.pixels, data: AssetData::embedded(proxy.data) });
    (s, id)
}

fn warnings(r: &Value) -> String {
    r["warnings"].to_string()
}

fn pdf_text(r: &Value) -> String {
    String::from_utf8_lossy(&crate::cmd::base64_decode(r["base64"].as_str().unwrap())).into_owned()
}

#[test]
fn a_missing_link_is_a_preflight_error_and_exports_its_proxy() {
    let dir = Dir::new("missing");
    let file = dir.0.join("art.png");
    let (mut s, _) = linked_session(&file, &png(40, 20, false));
    std::fs::remove_file(&file).unwrap();
    let issues = s.execute("preflight.run", &json!({})).unwrap();
    assert!(issues.to_string().contains("missingLink"), "{issues}");
    let r = s.execute("file.exportPdf", &json!({})).unwrap();
    assert!(warnings(&r).contains("art.png") && warnings(&r).contains("preview"), "{}", warnings(&r));
    assert!(pdf_text(&r).contains("/Subtype /Image") || pdf_text(&r).contains("/Subtype/Image"), "the proxy is in the PDF");
}

#[test]
fn a_modified_link_is_a_preflight_warning_and_exports_the_file_on_disk() {
    let dir = Dir::new("modified");
    let file = dir.0.join("art.png");
    let (mut s, _) = linked_session(&file, &png(40, 20, false));
    std::fs::write(&file, png(77, 33, false)).unwrap();
    let issues = s.execute("preflight.run", &json!({})).unwrap();
    assert!(issues.to_string().contains("modifiedLink"), "{issues}");
    let r = s.execute("file.exportPdf", &json!({})).unwrap();
    assert!(warnings(&r).contains("file on disk"), "{}", warnings(&r));
    let text = pdf_text(&r);
    assert!(text.contains("/Width 77") || text.contains("/Width%2077") || text.contains("/Width\n77"), "the new file's pixels are in the PDF");
}

#[test]
fn a_package_copies_a_linked_file_from_disk_and_links_to_the_copy() {
    let dir = Dir::new("package");
    let file = dir.0.join("art.png");
    let bytes = png(40, 20, false);
    let (mut s, id) = linked_session(&file, &bytes);
    let out = dir.0.join("out");
    s.execute("file.package", &json!({"dir": out.to_string_lossy(), "pdf": false, "idml": false})).unwrap();
    let copy = out.join("Links/art.png");
    assert_eq!(std::fs::read(&copy).unwrap(), bytes);
    let doc = std::fs::read_dir(&out)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "designcraft"))
        .unwrap();
    std::fs::remove_file(&file).unwrap();
    let mut t = Session::new();
    t.execute("file.open", &json!({"path": doc.to_string_lossy()})).unwrap();
    let a = &t.doc().unwrap().doc.assets[&id];
    assert_eq!((a.storage, a.link_relative.as_deref()), (Storage::Linked, Some("Links/art.png")));
    assert_eq!(a.data.load().unwrap().as_slice(), bytes.as_slice());
}

#[test]
fn opening_a_document_with_a_linked_alpha_wrap_image_reads_no_original() {
    let dir = Dir::new("alpha");
    let file = dir.0.join("art.png");
    let (mut s, _) = linked_session(&file, &png(64, 64, true));
    let image = s.doc().unwrap().doc.spreads[0].items[0].id.0;
    s.execute("object.textWrap", &json!({"ids": [image], "mode": "contour", "contour": "alphaChannel"})).unwrap();
    let path = dir.0.join("doc.designcraft");
    s.execute("file.save", &json!({"path": path.to_string_lossy()})).unwrap();
    let mut t = Session::new();
    t.execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap();
    let d = &t.doc().unwrap().doc;
    let a = d.assets.values().next().unwrap();
    assert!(a.data.is_linked() && a.data.resident().is_none(), "no original was read");
    let o = d.alpha_outlines.first().expect("the outline is traced from the proxy");
    assert_eq!((o.width, o.height), (64, 64), "scaled to the original's size");
}
