//! Paths stored in documents (links, data sources, book chapters) are followed only where
//! [`crate::io::document_path`] allows: never to another computer, and resolved beside the
//! document. Every path the engine hands the file system is recorded by [`crate::io::touched`].

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::Session;

const UNC: &str = r"\\192.0.2.1\share\x.png";
const FILE_URL: &str = "file://192.0.2.1/s/x.png";

fn png() -> Vec<u8> {
    designcraft_render::Rendered { width: 2, height: 2, pixels: vec![255; 16] }.to_png()
}

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dc-linked-paths-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A document saved as `doc` with one placed graphic per link, linked to it.
fn document_with_links(doc: &Path, links: &[&str]) -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    for (i, link) in links.iter().enumerate() {
        s.execute("file.place", &json!({"base64": crate::cmd::base64_encode(&png()), "name": format!("{i}.png"), "x": 72 * i, "y": 72})).unwrap();
        let link = link.to_string();
        s.edit(move |d, _| {
            let a = d.assets.values_mut().find(|a| a.link.is_none() && a.name == format!("{i}.png")).unwrap();
            std::sync::Arc::make_mut(a).link = Some(link);
            Ok(Value::Null)
        })
        .unwrap();
    }
    s.doc_mut().unwrap().path = Some(doc.to_string_lossy().into_owned());
    s
}

#[test]
fn links_to_other_computers_never_reach_the_file_system() {
    let dir = folder("remote");
    let doc = dir.join("doc.designcraft");
    let mut s = document_with_links(&doc, &[UNC, FILE_URL]);
    let (rows, seen) = crate::io::touched(|| {
        let rows = s.execute("links.list", &json!({})).unwrap();
        let issues = s.execute("preflight.run", &json!({})).unwrap();
        s.execute("links.relinkFolder", &json!({"extension": "jpg"})).unwrap();
        s.execute("links.update", &json!({})).unwrap();
        (rows, issues)
    });
    assert!(seen.is_empty(), "{seen:?}");
    let (rows, issues) = rows;
    for row in rows.as_array().unwrap() {
        assert_eq!(row["status"], "missing", "{row}");
        assert_eq!(row["refused"], "the path is on another computer", "{row}");
    }
    assert_eq!(issues.to_string().matches("Missing link").count(), 2, "{issues}");

    // Data sources and book chapters stored the same way.
    let mut d = s.doc().unwrap().doc.as_ref().clone();
    d.data_merge.sources.push(designcraft_doc::DataSource {
        id: 1,
        path: Some(UNC.replace("x.png", "data.csv")),
        relative_path: Some(FILE_URL.replace("x.png", "data.csv")),
        name: "data.csv".into(),
        delimiter: designcraft_doc::Delimiter::Comma,
        sheet: None,
        fields: vec![],
        rows: vec![],
        fingerprint: None,
        status: designcraft_doc::SourceStatus::Ok,
        warnings: vec![],
    });
    std::fs::write(&doc, designcraft_format::save(&d).unwrap()).unwrap();
    let mut opened = Session::new();
    let ((), seen) = crate::io::touched(|| {
        opened.execute("file.open", &json!({"path": doc.to_string_lossy()})).unwrap();
        opened.execute("links.list", &json!({})).unwrap();
    });
    // The document itself and the folders beside it, nothing else.
    assert!(seen.iter().all(|p| p.starts_with(&dir)), "{seen:?}");
    assert_eq!(opened.doc().unwrap().doc.data_merge.sources[0].status, designcraft_doc::SourceStatus::Missing);
    let book = dir.join("b.dcbook");
    std::fs::write(&book, json!({"documents": [UNC.replace("x.png", "c.designcraft"), FILE_URL.replace("x.png", "c.designcraft")]}).to_string())
        .unwrap();
    s.execute("book.open", &json!({"path": book.to_string_lossy()})).unwrap();
    let (list, seen) = crate::io::touched(|| s.execute("book.list", &json!({})).unwrap());
    assert!(seen.is_empty(), "{seen:?}");
    assert_eq!(list["documents"][0]["pages"], 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_relative_link_resolves_beside_the_document() {
    let dir = folder("relative");
    std::fs::write(dir.join("logo.png"), png()).unwrap();
    let mut s = document_with_links(&dir.join("doc.designcraft"), &["logo.png"]);
    let (rows, seen) = crate::io::touched(|| s.execute("links.list", &json!({})).unwrap());
    assert_eq!(rows[0]["status"], "ok", "{rows}");
    assert!(rows[0]["refused"].is_null());
    assert!(seen.iter().all(|p| p.starts_with(&dir)), "{seen:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
