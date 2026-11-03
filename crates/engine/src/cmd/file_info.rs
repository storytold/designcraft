//! File › File Info: the document's title, author, description, keywords and copyright. PDF
//! export writes the title, author, description and keywords into the file's metadata, and EPUB
//! export writes the title and author.

use designcraft_doc::FileInfo;
use designcraft_doc::file_info::{clean_keywords, parse_keywords};
use serde_json::{Map, Value, json};

use super::{CommandSpec, bad, cmd, has_doc};
use crate::{Result, Session};

const ID: &str = "file.info";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "file.info",
            "File Info…",
            ["File"],
            None,
            "{documentTitle?, author?, description?, keywords?: [\"a\", \"b\"] | \"a; b\", copyrightNotice?, copyrightInfoUrl?}: each field given replaces the stored one (\"\" or null clears it) → the file info",
            has_doc,
            file_info
        ),
        cmd!(query "file.info.get", "File Info", [], None, "{} → the file info, without changing the document", has_doc, |s, _| Ok(to_json(
            &s.doc()?.doc.file_info
        ))),
    ]
}

fn file_info(s: &mut Session, p: &Value) -> Result<Value> {
    let cur = s.doc()?.doc.file_info.clone();
    let o = match p {
        Value::Null => return Ok(to_json(&cur)),
        Value::Object(o) if o.is_empty() => return Ok(to_json(&cur)),
        Value::Object(o) => o,
        _ => return Err(bad(ID, "the parameters must be an object of fields")),
    };
    let next = apply(&cur, o)?;
    if next == cur {
        return Ok(to_json(&cur));
    }
    s.edit(|d, _| {
        d.file_info = next;
        Ok(to_json(&d.file_info))
    })
}

/// `cur` with the fields of `o` applied.
fn apply(cur: &FileInfo, o: &Map<String, Value>) -> Result<FileInfo> {
    let mut f = cur.clone();
    for (key, v) in o {
        let field = match key.as_str() {
            "documentTitle" => &mut f.document_title,
            "author" => &mut f.author,
            "description" => &mut f.description,
            "copyrightNotice" => &mut f.copyright_notice,
            "copyrightInfoUrl" => &mut f.copyright_info_url,
            "keywords" => {
                f.keywords = match v {
                    Value::Null => Vec::new(),
                    Value::String(t) => parse_keywords(t),
                    Value::Array(a) => {
                        let words: Option<Vec<&str>> = a.iter().map(Value::as_str).collect();
                        clean_keywords(words.ok_or_else(|| bad(ID, "`keywords` must be strings"))?)
                    }
                    _ => return Err(bad(ID, "`keywords` must be a list of strings or one string")),
                };
                continue;
            }
            other => return Err(bad(ID, format!("unknown field `{other}`"))),
        };
        *field = match v {
            Value::Null => String::new(),
            Value::String(t) => t.trim().to_string(),
            _ => return Err(bad(ID, format!("`{key}` must be a string"))),
        };
    }
    Ok(f)
}

fn to_json(f: &FileInfo) -> Value {
    json!({
        "documentTitle": f.document_title,
        "author": f.author,
        "description": f.description,
        "keywords": f.keywords,
        "copyrightNotice": f.copyright_notice,
        "copyrightInfoUrl": f.copyright_info_url,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    fn session() -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "title": "flyer-v3"})).unwrap();
        s
    }

    #[test]
    fn file_info_sets_reads_and_clears_the_fields() {
        let mut s = session();
        let empty = s.execute("file.info", &json!({})).unwrap();
        assert_eq!(empty["documentTitle"], "");
        assert_eq!(empty["keywords"], json!([]));
        let r = s
            .execute(
                "file.info",
                &json!({"documentTitle": " Spring Catalog ", "author": "Ana Ruiz", "description": "Shoes", "keywords": "spring; shoes, spring",
                    "copyrightNotice": "© 2026 Ana Ruiz", "copyrightInfoUrl": "https://example.com/rights"}),
            )
            .unwrap();
        assert_eq!(r["documentTitle"], "Spring Catalog");
        assert_eq!(r["keywords"], json!(["spring", "shoes"]));
        assert_eq!(s.execute("file.info", &json!({})).unwrap(), r, "a read returns what was set");
        let info = &s.doc().unwrap().doc.file_info;
        assert_eq!((info.author.as_str(), info.copyright_info_url.as_str()), ("Ana Ruiz", "https://example.com/rights"));
        assert_eq!(s.doc().unwrap().doc.title, "flyer-v3", "the document's name stays");
        // Only the fields given change; "" and null clear one.
        let r = s.execute("file.info", &json!({"author": "", "keywords": ["  catalog ", ""], "description": null})).unwrap();
        assert_eq!((r["author"].as_str(), r["description"].as_str(), r["documentTitle"].as_str()), (Some(""), Some(""), Some("Spring Catalog")));
        assert_eq!(r["keywords"], json!(["catalog"]));
    }

    #[test]
    fn file_info_is_one_undo_step_and_survives_a_save() {
        let mut s = session();
        let steps = |s: &Session| s.doc().unwrap().history.undo.len();
        let before = steps(&s);
        s.execute("file.info", &json!({"documentTitle": "Spring Catalog", "keywords": ["spring"]})).unwrap();
        assert_eq!(steps(&s), before + 1);
        s.execute("file.info", &json!({"documentTitle": "Spring Catalog"})).unwrap();
        s.execute("file.info", &json!({})).unwrap();
        assert_eq!(steps(&s), before + 1, "no change, no undo step");
        let saved = s.execute("file.serialize", &json!({})).unwrap();
        let mut t = Session::new();
        t.execute("file.openBytes", &json!({"name": "a.designcraft", "base64": saved["base64"]})).unwrap();
        assert_eq!(t.doc().unwrap().doc.file_info.document_title, "Spring Catalog", "saved with the document");
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.file_info.is_empty());
    }

    #[test]
    fn file_info_get_leaves_a_data_merge_preview_on() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let frame = s.execute("frame.create", &json!({"rect": [36, 36, 100, 100], "content": "text", "text": ""})).unwrap();
        let story = frame["story"].as_u64().unwrap();
        s.execute("data.source.select", &json!({"bytes": crate::cmd::base64_encode(b"x\nAda\n"), "name": "people.csv"})).unwrap();
        s.execute("data.placeholder.add", &json!({"field": "x", "story": story, "at": 0})).unwrap();
        s.execute("file.info", &json!({"author": "Ana Ruiz"})).unwrap();
        s.execute("data.preview", &json!({"record": 1})).unwrap();
        let r = s.execute("file.info.get", &json!({})).unwrap();
        assert_eq!(r["author"], "Ana Ruiz");
        assert!(s.doc().unwrap().preview_stash.is_some(), "the preview stays on");
        s.execute("file.info", &json!({})).unwrap();
        assert!(s.doc().unwrap().preview_stash.is_none(), "file.info is an edit, so it ends the preview");
    }

    #[test]
    fn file_info_refuses_bad_fields() {
        let mut s = session();
        for p in [json!({"title": "x"}), json!({"author": 3}), json!({"keywords": [1]}), json!({"keywords": {"a": 1}})] {
            assert!(s.execute("file.info", &p).is_err(), "{p}");
        }
        assert!(s.doc().unwrap().doc.file_info.is_empty(), "a refused call changes nothing");
        // Parameters that are not an object are refused; null reads like {}.
        s.execute("file.info", &json!({"author": "Ana Ruiz"})).unwrap();
        for p in [json!(3), json!("author"), json!(true), json!(["author"])] {
            let e = s.execute("file.info", &p).unwrap_err().to_string();
            assert!(e.contains("must be an object"), "{p}: {e}");
        }
        assert_eq!(s.execute("file.info", &serde_json::Value::Null).unwrap()["author"], "Ana Ruiz");
    }

    #[test]
    fn pdf_and_epub_export_take_the_file_info() {
        let mut s = session();
        s.execute("file.info", &json!({"documentTitle": "Spring Catalog", "author": "Ana Ruiz", "keywords": ["spring", "shoes"]})).unwrap();
        let r = s.execute("file.exportPdf", &json!({})).unwrap();
        let pdf = crate::cmd::base64_decode(r["base64"].as_str().unwrap());
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/Title(Spring Catalog)") && text.contains("/Author(Ana Ruiz)"), "PDF metadata");
        let r = s.execute("file.exportPdf", &json!({"title": "Proof"})).unwrap();
        let pdf = crate::cmd::base64_decode(r["base64"].as_str().unwrap());
        assert!(String::from_utf8_lossy(&pdf).contains("/Title(Proof)"), "a title given to the export wins");
        for (cmd, opf_path) in [("file.exportEpub", "OEBPS/content.opf"), ("file.exportFixedEpub", "OEBPS/content.opf")] {
            let r = s.execute(cmd, &json!({})).unwrap();
            let epub = crate::cmd::base64_decode(r["base64"].as_str().unwrap());
            let mut z = zip::ZipArchive::new(std::io::Cursor::new(epub)).unwrap();
            let mut opf = String::new();
            std::io::Read::read_to_string(&mut z.by_name(opf_path).unwrap(), &mut opf).unwrap();
            assert!(opf.contains("<dc:title>Spring Catalog</dc:title>") && opf.contains("<dc:creator>Ana Ruiz</dc:creator>"), "{cmd}: {opf}");
        }
    }
}
