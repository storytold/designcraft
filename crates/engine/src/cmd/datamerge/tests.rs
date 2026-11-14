//! Engine tests for data merge. They drive the commands, not a private copy of the parser.

use std::path::{Path, PathBuf};

use designcraft_doc::{Content, DataField, DataFieldKind, DataSource, Delimiter, HyperlinkDest, Selection, SourceStatus, StoryId, TextSel};
use serde_json::{Value, json};

use super::parse::{self, parse_csv};
use super::{read_path_capped, relative_between, resolve_sources_on_open};
use crate::Session;

fn scratch(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("dc-merge-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn write(path: &Path, body: &str) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(path, body).unwrap();
}

const PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

fn write_png(path: &Path) {
    let bytes = crate::cmd::base64_decode(PNG_B64);
    assert!(designcraft_render::image_size(&bytes).is_some(), "the fixture png must decode");
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(path, bytes).unwrap();
}

fn exec(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn err(s: &mut Session, id: &str, p: Value) -> String {
    s.execute(id, &p).unwrap_err().to_string()
}

fn texts(s: &Session) -> Vec<String> {
    let mut v: Vec<String> = s.documents()[s.active_index().unwrap()].doc.stories.values().map(|st| st.text.clone()).collect();
    v.sort();
    v
}

fn origins(s: &Session) -> Vec<(i32, i32)> {
    let doc = &s.documents()[s.active_index().unwrap()].doc;
    let mut v = Vec::new();
    for sp in &doc.spreads {
        for it in &sp.items {
            let b = it.bounds();
            v.push((b.y0.round() as i32, b.x0.round() as i32));
        }
    }
    v.sort();
    v
}

fn new_doc(s: &mut Session, pages: u32) {
    exec(s, "file.new", json!({"facingPages": false, "pages": pages}));
}

fn text_frame(s: &mut Session, rect: [f64; 4], text: &str, spread: Option<u32>) -> Value {
    let mut p = json!({"rect": rect, "content": "text", "text": text});
    if let Some(spread) = spread {
        p["spread"] = json!(spread);
    }
    exec(s, "frame.create", p)
}

fn select_csv(s: &mut Session, body: &str) -> Value {
    exec(s, "data.source.select", json!({"bytes": crate::cmd::base64_encode(body.as_bytes()), "name": "people.csv"}))
}

#[test]
fn csv_parsing() {
    let r = parse_csv("Name,Title\n\"Ada, Countess\",\"Analyst \"\"No. 1\"\"\"\r\nGrace,Admiral\n");
    assert_eq!(r, vec![vec!["Name", "Title"], vec!["Ada, Countess", "Analyst \"No. 1\""], vec!["Grace", "Admiral"]]);
    let quoted = parse_csv("Name\n\"Ada\nGrace\"\n");
    assert_eq!(quoted[1][0], "Ada\nGrace");
}

#[test]
fn data_source_sniffs_quoted_logical_headers() {
    for (body, delimiter, names) in [
        ("\"Last, First\";City\nAda;London\n", Delimiter::Semicolon, ["Last, First", "City"]),
        ("\"Name\tLabel\",City\nAda,London\n", Delimiter::Comma, ["Name\tLabel", "City"]),
        ("\"Name\nLabel\";City\nAda;London\n", Delimiter::Semicolon, ["Name\nLabel", "City"]),
        ("\r\n \r\nName;City\r\nAda;London\r\n", Delimiter::Semicolon, ["Name", "City"]),
        ("\"Name \"\"label,\"\"\";City\nAda;London\n", Delimiter::Semicolon, ["Name \"label,\"", "City"]),
    ] {
        let mut s = Session::new();
        new_doc(&mut s, 1);
        let result = exec(&mut s, "data.source.select", json!({"bytes": crate::cmd::base64_encode(body.as_bytes()), "name": "people.txt"}));
        assert_eq!(result["records"], 1, "{body:?}");
        let source = &s.documents()[0].doc.data_merge.sources[0];
        assert_eq!(source.delimiter, delimiter, "{body:?}");
        assert_eq!(source.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), names, "{body:?}");
        assert_eq!(source.rows, vec![vec!["Ada", "London"]], "{body:?}");
    }
}

#[test]
fn data_source_sniff_quote_state_belongs_to_the_candidate_delimiter() {
    for ending in ["\n", "\r\n", "\r"] {
        for (body, delimiter, names) in [
            (format!("Name,\"preferred\tCity\"{ending}Ada\tLondon{ending}"), Delimiter::Tab, ["Name,\"preferred", "City\""]),
            (format!("\"Last, First\";City{ending}Ada;London{ending}"), Delimiter::Semicolon, ["Last, First", "City"]),
            (format!("\"Name\tLabel\",City{ending}Ada,London{ending}"), Delimiter::Comma, ["Name\tLabel", "City"]),
            (format!("\"\";\"\"{ending} \t{ending}\"Last, First\";City{ending}Ada;London{ending}"), Delimiter::Semicolon, ["Last, First", "City"]),
        ] {
            let automatic = parse::table_from_bytes(body.as_bytes(), "people.txt", None).unwrap();
            let explicit = parse::table_from_bytes(body.as_bytes(), "people.txt", Some(delimiter)).unwrap();
            let fields: Vec<_> = names.into_iter().map(|name| DataField { name: name.into(), kind: DataFieldKind::Text }).collect();
            assert_eq!(automatic.delimiter, delimiter, "{body:?}");
            assert_eq!(automatic.fields, fields, "{body:?}");
            assert_eq!(automatic.rows, vec![vec!["Ada", "London"]], "{body:?}");
            assert!(automatic.warnings.is_empty(), "{body:?}: {:?}", automatic.warnings);
            assert_eq!(automatic, explicit, "{body:?}");
        }
    }
}

#[test]
fn data_source_sniff_prefers_coherent_quoted_field_boundaries() {
    for ending in ["\n", "\r\n", "\r"] {
        for (header, first, second, delimiter, names, rows, warnings) in [
            (
                "\"Name\";\"Last, First\"",
                "\"Ada\";\"Lovelace, Countess\"",
                "\"Grace\";\"Hopper, Admiral\"",
                Delimiter::Semicolon,
                vec!["Name", "Last, First"],
                vec![vec!["Ada", "Lovelace, Countess"], vec!["Grace", "Hopper, Admiral"]],
                vec![],
            ),
            (
                "\"Name\",\"Region\tCity\"",
                "\"Ada\",\"London\"",
                "\"Grace\",\"Paris\"",
                Delimiter::Comma,
                vec!["Name", "Region\tCity"],
                vec![vec!["Ada", "London"], vec!["Grace", "Paris"]],
                vec![],
            ),
            (
                "\"Name\";\"City\";\"Last, \"\"First\"\"\";\"Title\"",
                "\"Ada\";\"London\";\"Lovelace, \"\"Countess\"\"\";\"Analyst\"",
                "\"Grace\";\"Paris\";\"Hopper, \"\"Admiral\"\"\";\"Engineer\";\"ignored\"",
                Delimiter::Semicolon,
                vec!["Name", "City", "Last, \"First\"", "Title"],
                vec![vec!["Ada", "London", "Lovelace, \"Countess\"", "Analyst"], vec!["Grace", "Paris", "Hopper, \"Admiral\"", "Engineer"]],
                vec!["Row 3 has cells past the header. Those cells were ignored."],
            ),
            (
                "\"Name\",\"Role\",\"Region\t\"\"City\"\"\",\"Title\"",
                "\"Ada\",\"Analyst\",\"London\t\"\"West End\"\"\",\"Countess\"",
                "\"Grace\",\"Engineer\",\"Paris\t\"\"Centre\"\"\",\"Admiral\",\"ignored\"",
                Delimiter::Comma,
                vec!["Name", "Role", "Region\t\"City\"", "Title"],
                vec![vec!["Ada", "Analyst", "London\t\"West End\"", "Countess"], vec!["Grace", "Engineer", "Paris\t\"Centre\"", "Admiral"]],
                vec!["Row 3 has cells past the header. Those cells were ignored."],
            ),
        ] {
            let body = format!("{header}{ending}{first}{ending}{second}{ending}");
            let automatic = parse::table_from_bytes(body.as_bytes(), "people.txt", None).unwrap();
            let explicit = parse::table_from_bytes(body.as_bytes(), "people.txt", Some(delimiter)).unwrap();
            let fields: Vec<_> = names.into_iter().map(|name| DataField { name: name.into(), kind: DataFieldKind::Text }).collect();
            assert_eq!(automatic.delimiter, delimiter, "{body:?}");
            assert_eq!(automatic.fields, fields, "{body:?}");
            assert_eq!(automatic.rows, rows, "{body:?}");
            assert_eq!(automatic.warnings, warnings, "{body:?}");
            assert_eq!(automatic, explicit, "{body:?}");
        }
    }
}

#[test]
fn data_source_sniff_retains_priority_and_permissive_fallback() {
    for ending in ["\n", "\r\n", "\r"] {
        for (header, row, delimiter, names, cells) in [
            // Equally coherent candidates retain Tab, then Comma, then Semicolon priority.
            ("Name,Title\tCity", "Ada,Analyst\tLondon\tignored", Delimiter::Tab, ["Name,Title", "City"], ["Ada,Analyst", "London"]),
            ("Name;Title,City", "Ada;Analyst,London,ignored", Delimiter::Comma, ["Name;Title", "City"], ["Ada;Analyst", "London"]),
            // No coherent candidate separates fields here. Keep parsing the trailing text
            // after a quoted field exactly as before, using the same delimiter priority.
            (
                "\"Name\"suffix,Title\tCity",
                "\"Ada\"suffix,Analyst\tLondon\tignored",
                Delimiter::Tab,
                ["Namesuffix,Title", "City"],
                ["Adasuffix,Analyst", "London"],
            ),
            (
                "\"Name\"suffix;Title,City",
                "\"Ada\"suffix;Analyst,London,ignored",
                Delimiter::Comma,
                ["Namesuffix;Title", "City"],
                ["Adasuffix;Analyst", "London"],
            ),
            ("\"Name\"suffix;City", "\"Ada\"suffix;London;ignored", Delimiter::Semicolon, ["Namesuffix", "City"], ["Adasuffix", "London"]),
        ] {
            let body = format!("{header}{ending}{row}{ending}");
            let automatic = parse::table_from_bytes(body.as_bytes(), "people.txt", None).unwrap();
            let explicit = parse::table_from_bytes(body.as_bytes(), "people.txt", Some(delimiter)).unwrap();
            let fields: Vec<_> = names.into_iter().map(|name| DataField { name: name.into(), kind: DataFieldKind::Text }).collect();
            assert_eq!(automatic.delimiter, delimiter, "{body:?}");
            assert_eq!(automatic.fields, fields, "{body:?}");
            assert_eq!(automatic.rows, vec![cells.to_vec()], "{body:?}");
            assert_eq!(automatic.warnings, vec!["Row 2 has cells past the header. Those cells were ignored."], "{body:?}");
            assert_eq!(automatic, explicit, "{body:?}");
        }
    }
}

#[test]
fn data_source_sniff_allows_post_quote_whitespace() {
    for ending in ["\n", "\r\n", "\r"] {
        for space in [" ", "\u{00A0}", "\u{2003}"] {
            for (header, separator, delimiter, names) in [
                (format!("\"Name\"{space};\"Last, First\""), ';', Delimiter::Semicolon, ["Name", "Last, First"]),
                (format!("\"Name\"{space},\"Region\tCity\"{space}"), ',', Delimiter::Comma, ["Name", "Region\tCity"]),
                (format!("\"Name\"{space}\t\"Last, First; City\"{space}"), '\t', Delimiter::Tab, ["Name", "Last, First; City"]),
            ] {
                let body = format!(
                    "{header}{ending}\"Ada\"{space}{separator}\"London\"{space}{ending}\"Grace\"{space}{separator}\"Paris\"{space}{separator}ignored{ending}"
                );
                let automatic = parse::table_from_bytes(body.as_bytes(), "people.txt", None).unwrap();
                let explicit = parse::table_from_bytes(body.as_bytes(), "people.txt", Some(delimiter)).unwrap();
                let fields: Vec<_> = names.into_iter().map(|name| DataField { name: name.into(), kind: DataFieldKind::Text }).collect();
                assert_eq!(automatic.delimiter, delimiter, "{body:?}");
                assert_eq!(automatic.fields, fields, "{body:?}");
                // Header classification trims; record values must keep their whitespace.
                assert_eq!(
                    automatic.rows,
                    vec![vec![format!("Ada{space}"), format!("London{space}")], vec![format!("Grace{space}"), format!("Paris{space}")]],
                    "{body:?}"
                );
                assert_eq!(automatic.warnings, vec!["Row 3 has cells past the header. Those cells were ignored."], "{body:?}");
                assert_eq!(automatic, explicit, "{body:?}");
            }
        }
    }
}

#[test]
fn data_source_sniff_post_quote_whitespace_keeps_closed_field_state() {
    for ending in ["\n", "\r\n", "\r"] {
        for space in [" ", "\u{00A0}", "\u{2003}"] {
            for suffix in ["suffix", "\"suffix\""] {
                let body = format!("\"Name\"{space}{suffix};\"Last, First\"{ending}Ada,London,ignored{ending}");
                // Both separated formats have incompatible post-quote suffixes. Keep
                // the permissive Comma fallback, including literal quotes in the name.
                let automatic = parse::table_from_bytes(body.as_bytes(), "people.txt", None).unwrap();
                let explicit = parse::table_from_bytes(body.as_bytes(), "people.txt", Some(Delimiter::Comma)).unwrap();
                assert_eq!(automatic.delimiter, Delimiter::Comma, "{body:?}");
                assert_eq!(
                    automatic.fields,
                    vec![
                        DataField { name: format!("Name{space}{suffix};\"Last"), kind: DataFieldKind::Text },
                        DataField { name: "First\"".into(), kind: DataFieldKind::Text },
                    ],
                    "{body:?}"
                );
                assert_eq!(automatic.rows, vec![vec!["Ada", "London"]], "{body:?}");
                assert_eq!(automatic.warnings, vec!["Row 2 has cells past the header. Those cells were ignored."], "{body:?}");
                assert_eq!(automatic, explicit, "{body:?}");
            }
        }
    }
}

#[test]
fn data_source_literal_quote_header_merges_the_detected_column() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    text_frame(&mut s, [36.0, 36.0, 240.0, 80.0], "", None);
    let body = "Name,\"preferred\tCity\"\nAda\tLondon\nGrace\tParis\n";
    let selected = exec(&mut s, "data.source.select", json!({"bytes": crate::cmd::base64_encode(body.as_bytes()), "name": "people.txt"}));
    assert_eq!(selected["records"], 2);
    let source = &s.documents()[0].doc.data_merge.sources[0];
    assert_eq!(source.delimiter, Delimiter::Tab);
    assert_eq!(source.rows, vec![vec!["Ada", "London"], vec!["Grace", "Paris"]]);
    exec(&mut s, "data.placeholder.add", json!({"field": "City\""}));
    let merged = exec(&mut s, "data.merge", json!({}));
    assert_eq!(merged["records"], 2);
    assert_eq!(merged["pages"], 2);
    assert_eq!(texts(&s), vec!["London", "Paris"]);
}

#[test]
fn data_source_preserves_quoted_newlines_and_counts_record_lines() {
    for ending in ["\n", "\r\n", "\r"] {
        let body = format!("Name,City{ending}\"Ada{ending}Lovelace\",London{ending}Grace,Paris,extra{ending}");
        let mut s = Session::new();
        new_doc(&mut s, 1);
        let result = select_csv(&mut s, &body);
        assert_eq!(result["records"], 2, "{ending:?}");
        let source = &s.documents()[0].doc.data_merge.sources[0];
        assert_eq!(source.rows, vec![vec![format!("Ada{ending}Lovelace"), "London".into()], vec!["Grace".into(), "Paris".into()]]);
        assert_eq!(source.warnings.len(), 1);
        assert!(source.warnings[0].contains("Row 4"), "{:?}", source.warnings);
    }
}

#[test]
fn data_source_quoted_header_and_cr_records_merge_independently() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    text_frame(&mut s, [36.0, 36.0, 240.0, 80.0], "", None);
    let body = "\u{FEFF}\"Last, First\";City\rAda;London\rGrace;Paris\r";
    exec(&mut s, "data.source.select", json!({"bytes": crate::cmd::base64_encode(body.as_bytes()), "name": "people.txt"}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Last, First"}));
    let template = s.documents()[0].doc.clone();
    let result = exec(&mut s, "data.merge", json!({}));
    assert_eq!(result["records"], 2);
    assert_eq!(result["pages"], 2);
    assert_eq!(texts(&s), vec!["Ada", "Grace"]);
    assert_eq!(serde_json::to_value(&s.documents()[0].doc).unwrap(), serde_json::to_value(&template).unwrap());
}

#[test]
fn data_source_discarded_quoted_fields_keep_records_and_warning_lines() {
    for ending in ["\n", "\r\n", "\r"] {
        for (discarded, following_line) in [(format!("\"Address \"\"line 1\"\"{ending}Address line 2\""), 4), ("literal\"quote".into(), 3)] {
            let mut s = Session::new();
            new_doc(&mut s, 1);
            text_frame(&mut s, [36.0, 36.0, 240.0, 80.0], "", None);
            let body = format!("Name{ending}Ada,,,{discarded}{ending}Grace,extra{ending}");
            let selected = select_csv(&mut s, &body);
            assert_eq!(selected["records"], 2, "{body:?}");
            let source = &s.documents()[0].doc.data_merge.sources[0];
            assert_eq!(source.rows, vec![vec!["Ada"], vec!["Grace"]], "{body:?}");
            assert_eq!(source.warnings.len(), 2, "{:?}", source.warnings);
            assert!(source.warnings[0].contains("Row 2"), "{:?}", source.warnings);
            assert!(source.warnings[1].contains(&format!("Row {following_line}")), "{:?}", source.warnings);
            exec(&mut s, "data.placeholder.add", json!({"field": "Name"}));
            let merged = exec(&mut s, "data.merge", json!({}));
            assert_eq!(merged["records"], 2);
            assert_eq!(merged["pages"], 2);
            assert_eq!(texts(&s), vec!["Ada", "Grace"]);
        }
    }
}

#[test]
fn data_source_delimiter_override_and_known_extensions_win() {
    let body = "Name;City\nAda;London\n";
    let csv = parse::table_from_bytes(body.as_bytes(), "people.csv", None).unwrap();
    assert_eq!(csv.delimiter, Delimiter::Comma);
    assert_eq!(csv.rows, vec![vec!["Ada;London"]]);
    let explicit = parse::table_from_bytes(body.as_bytes(), "people.csv", Some(Delimiter::Semicolon)).unwrap();
    assert_eq!(explicit.rows, vec![vec!["Ada", "London"]]);
    let tab = parse::table_from_bytes(b"Name,Title\tCity\nAda,Analyst\tLondon", "people.tsv", None).unwrap();
    assert_eq!(tab.delimiter, Delimiter::Tab);
    assert_eq!(tab.rows, vec![vec!["Ada,Analyst", "London"]]);
}

#[test]
fn text_sniff_padding_skips_and_warnings() {
    let dir = scratch("sniff");
    write(&dir.join("tab.txt"), "Name\tCity\nAda\tTown\n");
    write(&dir.join("semi.txt"), "Name;City\nAda;Town\n");
    write(&dir.join("comma.txt"), "Name,City\nAda,Town\n");
    write(&dir.join("people.tsv"), "Name\tCity\nAda\tTown\n");
    write(&dir.join("forced.csv"), "Name;City\nAda;Town\n");
    let mut s = Session::new();
    new_doc(&mut s, 1);
    let row = |s: &Session| s.documents()[s.active_index().unwrap()].doc.data_merge.sources[0].rows[0].clone();
    exec(&mut s, "data.source.select", json!({"path": dir.join("tab.txt")}));
    assert_eq!(row(&s), vec!["Ada", "Town"]);
    exec(&mut s, "data.source.select", json!({"path": dir.join("semi.txt")}));
    assert_eq!(row(&s), vec!["Ada", "Town"]);
    exec(&mut s, "data.source.select", json!({"path": dir.join("comma.txt")}));
    assert_eq!(row(&s), vec!["Ada", "Town"]);
    exec(&mut s, "data.source.select", json!({"path": dir.join("people.tsv")}));
    assert_eq!(row(&s), vec!["Ada", "Town"]);
    exec(&mut s, "data.source.select", json!({"path": dir.join("forced.csv"), "delimiter": "semicolon"}));
    assert_eq!(row(&s), vec!["Ada", "Town"]);

    let short = select_csv(&mut s, "Name,City\nAda\n\nGrace,Paris,extra\n");
    assert!(short["warnings"][0].as_str().unwrap().contains("Row 4"), "{}", short["warnings"]);
    let src = &s.documents()[0].doc.data_merge.sources[0];
    assert_eq!(src.rows, vec![vec!["Ada", ""], vec!["Grace", "Paris"]]);
    assert!(src.warnings[0].contains("Row 4"));
}

#[test]
fn kind_marks_and_hard_errors_do_not_attach() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    let marked = select_csv(&mut s, "@Photo,#Code,Name\npic.png,abc,Ada\n");
    assert_eq!(
        marked["fields"],
        json!([
            {"name": "Photo", "kind": "image"},
            {"name": "Code", "kind": "qr"},
            {"name": "Name", "kind": "text"}
        ])
    );
    assert_eq!(marked["records"], 1);

    let bad = scratch("bad").join("bad.csv");
    std::fs::write(&bad, [0xFFu8, 0xFE, 0x00]).unwrap();
    let msg = err(&mut s, "data.source.select", json!({"path": bad}));
    assert!(msg.contains("not UTF-8"), "{msg}");
    assert_eq!(s.documents()[0].doc.data_merge.sources[0].fields[0].name, "Photo");

    assert!(err(&mut s, "data.source.select", json!({"bytes": crate::cmd::base64_encode(b",City\nAda,Town\n"), "name": "e.csv"})).contains("empty"));
    assert!(
        err(&mut s, "data.source.select", json!({"bytes": crate::cmd::base64_encode(b"@Name,Name\na,b\n"), "name": "e.csv"})).contains("duplicate")
    );
    assert_eq!(s.documents()[0].doc.data_merge.sources.len(), 1);

    let mut wide = String::from("A");
    for i in 0..200 {
        wide.push(',');
        wide.push_str(&format!("C{i}"));
    }
    wide.push('\n');
    assert!(err(&mut s, "data.source.select", json!({"bytes": crate::cmd::base64_encode(wide.as_bytes()), "name": "wide.csv"})).contains("200"));

    let mut rows = String::from("Name\n");
    for _ in 0..100_001 {
        rows.push_str("a\n");
    }
    assert!(err(&mut s, "data.source.select", json!({"bytes": crate::cmd::base64_encode(rows.as_bytes()), "name": "big.csv"})).contains("100000"));
    assert_eq!(s.documents()[0].doc.data_merge.sources[0].rows.len(), 1, "a failed select keeps the previous source");

    assert!(err(&mut s, "data.source.select", json!({"bytes": crate::cmd::base64_encode(b"Name\nAda\n"), "name": "book.xls"})).contains("xls"));
    assert!(err(&mut s, "data.source.update", json!({})).contains("no file") || err(&mut s, "data.source.update", json!({})).contains("no file"));
}

fn xlsx(parts: &[(&str, &str)]) -> String {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let o = zip::write::SimpleFileOptions::default();
        for (name, body) in parts {
            z.start_file(*name, o).unwrap();
            std::io::Write::write_all(&mut z, body.as_bytes()).unwrap();
        }
        z.finish().unwrap();
    }
    crate::cmd::base64_encode(&buf.into_inner())
}

#[test]
fn excel_first_sheet_named_sheet_and_cached_values() {
    let first = xlsx(&[
        (
            "xl/workbook.xml",
            r#"<workbook xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Prices" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        ),
        ("xl/_rels/workbook.xml.rels", r#"<Relationships><Relationship Id="rId1" Type="x" Target="worksheets/sheet1.xml"/></Relationships>"#),
        ("xl/sharedStrings.xml", r#"<sst><si><t>Item</t></si><si><t>Price</t></si><si><t>Tea</t></si></sst>"#),
        (
            "xl/worksheets/sheet1.xml",
            r#"<worksheet><sheetData><row r="2"><c r="B2" t="s"><v>0</v></c><c r="C2" t="s"><v>1</v></c></row><row r="3"><c r="B3" t="s"><v>2</v></c><c r="C3"><v>3.5</v></c></row><row r="4"><c r="B4" t="inlineStr"><is><t>Cake</t></is></c><c r="C4"><v>12.0</v></c></row></sheetData></worksheet>"#,
        ),
    ]);
    let mut s = Session::new();
    new_doc(&mut s, 1);
    exec(&mut s, "data.source.select", json!({"bytes": first, "name": "prices.xlsx"}));
    let src = &s.documents()[0].doc.data_merge.sources[0];
    assert_eq!(src.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), vec!["Item", "Price"]);
    assert_eq!(src.rows[0], vec!["Tea", "3.5"]);
    assert_eq!(src.rows[1][1], "12");

    let book = xlsx(&[
        (
            "xl/workbook.xml",
            r#"<workbook xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Ignore" sheetId="1" r:id="rId1"/><sheet name="People" sheetId="2" r:id="rId2"/></sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<Relationships><Relationship Id="rId1" Type="x" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="x" Target="worksheets/sheet2.xml"/></Relationships>"#,
        ),
        (
            "xl/worksheets/sheet1.xml",
            r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Nope</t></is></c></row></sheetData></worksheet>"#,
        ),
        (
            "xl/worksheets/sheet2.xml",
            r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Name</t></is></c><c r="B1" t="inlineStr"><is><t>Qty</t></is></c><c r="C1" t="inlineStr"><is><t>On</t></is></c><c r="D1" t="inlineStr"><is><t>When</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>Ada</t></is></c><c r="B2"><v>12.0</v></c><c r="C2" t="b"><v>1</v></c><c r="D2"><v>44927</v></c></row><row r="3"><c r="A3" t="inlineStr"><is><t>Grace</t></is></c><c r="C3"><f>1+1</f></c></row></sheetData></worksheet>"#,
        ),
    ]);
    exec(&mut s, "data.source.select", json!({"bytes": book, "name": "people.xlsx", "sheet": "People"}));
    let src = &s.documents()[0].doc.data_merge.sources[0];
    assert_eq!(src.rows[0], vec!["Ada", "12", "TRUE", "44927"]);
    assert_eq!(src.rows[1], vec!["Grace", "", "", ""]);
    let msg = err(&mut s, "data.source.select", json!({"bytes": book, "name": "people.xlsx", "sheet": "Missing"}));
    assert!(msg.contains("Missing"), "{msg}");
    assert_eq!(s.documents()[0].doc.data_merge.sources[0].rows[0][0], "Ada");
}

#[test]
fn typed_fields_keep_style_and_unknown_markers() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    select_csv(&mut s, "Title,Name\nAdmiral,Ada\n");
    let made = text_frame(&mut s, [36.0, 36.0, 400.0, 80.0], "<<Title>> and <<Nope>>", None);
    exec(&mut s, "text.select", json!({"story": made["story"], "anchor": 0, "focus": 0}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Name"}));
    let story = s.documents()[0].doc.stories.values().next().unwrap();
    let marker = story.text.find("<<Name>>").unwrap();
    exec(&mut s, "text.select", json!({"story": made["story"], "anchor": marker, "focus": marker + "<<Name>>".len()}));
    exec(&mut s, "type.char", json!({"size": 21}));
    exec(&mut s, "data.merge", json!({}));
    let doc = &s.documents()[s.active_index().unwrap()].doc;
    let story = doc.stories.values().next().unwrap();
    assert_eq!(story.text, "AdaAdmiral and <<Nope>>");
    let at = story.text.find("Ada").unwrap();
    assert_eq!(story.format_after(at).over.size, Some(21.0));
}

/// Every record stat'd and read its image again, so a sheet naming one file in many rows read it
/// once per row -- up to 256 MiB each time (#1595). A merge reads a file once and the records
/// share the bytes.
#[test]
fn one_image_named_by_many_records_is_read_once() {
    let dir = scratch("image-reads");
    let data = dir.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let shared = dir.join("shared.png");
    write_png(&shared);
    // 25 records, all naming the same file.
    let rows: String = (0..25).map(|_| format!("{}\n", shared.display())).collect();
    write(&data.join("many.csv"), &format!("@Photo\n{rows}"));

    let mut s = Session::new();
    new_doc(&mut s, 1);
    exec(&mut s, "data.source.select", json!({"path": data.join("many.csv")}));
    let frame = exec(&mut s, "frame.create", json!({"rect": [36.0, 36.0, 80.0, 80.0], "content": "graphic"}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Photo", "item": frame["id"]}));
    let r = exec(&mut s, "data.merge", json!({}));
    assert_eq!(r["records"], 25);
    assert_eq!(r["missingImages"].as_array().map(Vec::len).unwrap_or(0), 0, "every record found the image");

    // Each record has its graphic, and they share one buffer: the document holds the file's bytes
    // once however many records named it.
    let doc = &s.documents()[s.active_index().unwrap()].doc;
    let graphics: Vec<_> = doc
        .spreads
        .iter()
        .flat_map(|sp| &sp.items)
        .filter_map(|it| match &it.content {
            Content::Graphic(g) => Some(g.asset),
            _ => None,
        })
        .collect();
    assert_eq!(graphics.len(), 25, "one graphic per record");
}

/// The same merge, counting what the engine actually hands the file system: `io::touched` records
/// every path read, so the reads are counted rather than inferred. The document already shares
/// identical asset bytes, so memory alone does not show this -- the I/O is the bug (#1595).
#[test]
fn a_merge_reads_a_repeated_image_once_not_once_per_record() {
    let dir = scratch("image-read-count");
    let data = dir.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let shared = dir.join("shared.png");
    write_png(&shared);
    let rows: String = (0..25).map(|_| format!("{}\n", shared.display())).collect();
    write(&data.join("many.csv"), &format!("@Photo\n{rows}"));

    let mut s = Session::new();
    new_doc(&mut s, 1);
    exec(&mut s, "data.source.select", json!({"path": data.join("many.csv")}));
    let frame = exec(&mut s, "frame.create", json!({"rect": [36.0, 36.0, 80.0, 80.0], "content": "graphic"}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Photo", "item": frame["id"]}));

    let (r, seen) = crate::io::touched(|| exec(&mut s, "data.merge", json!({})));
    assert_eq!(r["records"], 25);
    let reads = seen.iter().filter(|p| p.ends_with("shared.png")).count();
    assert!(reads <= 2, "the image is read once for the merge, not once per record: {reads} reads for 25 records");
}

#[test]
fn images_resolve_in_order_and_roles_win() {
    let dir = scratch("images");
    let data = dir.join("data");
    let docs = dir.join("docs");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&docs).unwrap();
    let abs = dir.join("abs.png");
    let beside_data = data.join("beside.png");
    let beside_doc = docs.join("docpic.png");
    write_png(&abs);
    write_png(&beside_data);
    write_png(&beside_doc);
    write(
        &data.join("people.csv"),
        &format!("@Photo,Web,Note\n{},https://example.com/a.png,kept\nmissing.png,https://example.com/b.png,kept\n", abs.display()),
    );

    let mut s = Session::new();
    new_doc(&mut s, 1);
    exec(&mut s, "data.source.select", json!({"path": data.join("people.csv")}));
    let photo = exec(&mut s, "frame.create", json!({"rect": [36.0, 36.0, 80.0, 80.0], "content": "graphic"}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Photo", "item": photo["id"]}));
    let web = exec(&mut s, "frame.create", json!({"rect": [100.0, 36.0, 140.0, 80.0], "content": "graphic"}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Web", "role": "image", "item": web["id"]}));
    let r = exec(&mut s, "data.merge", json!({}));
    assert_eq!(r["records"], 2);
    assert_eq!(r["missingImages"].as_array().unwrap().len(), 3, "second photo, and both https cells");
    let doc = &s.documents()[s.active_index().unwrap()].doc;
    let mut linked = 0;
    let mut empty = 0;
    for sp in &doc.spreads {
        for it in &sp.items {
            match &it.content {
                Content::Graphic(g) => {
                    let asset = doc.assets.get(&g.asset).unwrap();
                    assert!(asset.link.as_ref().is_some_and(|p| p.contains("abs.png") || p.contains("beside.png") || p.contains("docpic.png")));
                    assert!(!asset.data.is_empty());
                    linked += 1;
                }
                Content::Unassigned => empty += 1,
                _ => {}
            }
        }
    }
    assert_eq!(linked, 1);
    assert!(empty >= 3);

    // Beside the data file, then beside the saved document.
    let mut s = Session::new();
    new_doc(&mut s, 1);
    write(&data.join("side.csv"), "Photo\nbeside.png\n");
    exec(&mut s, "data.source.select", json!({"path": data.join("side.csv")}));
    let frame = exec(&mut s, "frame.create", json!({"rect": [0.0, 0.0, 200.0, 100.0], "content": "graphic"}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Photo", "role": "image", "item": frame["id"]}));
    exec(&mut s, "data.merge", json!({}));
    assert!(graphic_link(&s).is_some_and(|p| p.contains("beside.png")));

    let mut s = Session::new();
    new_doc(&mut s, 1);
    write(&data.join("doc.csv"), "Photo\ndocpic.png\n");
    exec(&mut s, "data.source.select", json!({"path": data.join("doc.csv")}));
    exec(&mut s, "file.save", json!({"path": docs.join("layout.designcraft")}));
    let frame = exec(&mut s, "frame.create", json!({"rect": [0.0, 0.0, 200.0, 100.0], "content": "graphic"}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Photo", "role": "image", "item": frame["id"]}));
    exec(&mut s, "data.options", json!({"center": false, "linkImages": true, "fitting": "fitProportionally"}));
    exec(&mut s, "data.merge", json!({}));
    assert!(graphic_link(&s).is_some_and(|p| p.contains("docpic.png")));
    let (frame_box, placed) = graphic_boxes(&s);
    assert!((placed.x0 - frame_box.x0).abs() < 1.0, "center off keeps the fit at the top left");

    exec(&mut s, "file.activate", json!({"index": 0}));
    exec(&mut s, "data.options", json!({"center": true, "linkImages": false}));
    exec(&mut s, "data.merge", json!({}));
    let doc = &s.documents()[s.active_index().unwrap()].doc;
    let asset_link_none = doc.assets.values().any(|a| a.link.is_none() && !a.data.is_empty());
    assert!(asset_link_none);
    let (frame_box, placed) = graphic_boxes(&s);
    assert!((placed.center().x - frame_box.center().x).abs() < 1.0);
    assert!(placed.x0 > frame_box.x0 + 1.0);
}

fn graphic_link(s: &Session) -> Option<String> {
    let doc = &s.documents()[s.active_index().unwrap()].doc;
    for sp in &doc.spreads {
        for it in &sp.items {
            if let Content::Graphic(g) = &it.content {
                return doc.assets.get(&g.asset).and_then(|a| a.link.clone());
            }
        }
    }
    None
}

fn graphic_boxes(s: &Session) -> (designcraft_geom::Rect, designcraft_geom::Rect) {
    let doc = &s.documents()[s.active_index().unwrap()].doc;
    for sp in &doc.spreads {
        for it in &sp.items {
            if let Content::Graphic(g) = &it.content {
                let bb = g.xf.transform_rect_bbox(designcraft_geom::Rect::new(0.0, 0.0, g.size.0, g.size.1));
                return (it.inner_bounds(), bb);
            }
        }
    }
    panic!("no graphic");
}

#[test]
fn role_overrides_column_kind_qr_and_hyperlink() {
    let dir = scratch("roles");
    let pic = dir.join("pic.png");
    write_png(&pic);
    let mut s = Session::new();
    new_doc(&mut s, 1);
    select_csv(&mut s, &format!("Name,@Photo\n{},pic.png\n", pic.display()));
    let frame = exec(&mut s, "frame.create", json!({"rect": [10.0, 10.0, 80.0, 80.0], "content": "graphic"}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Name", "role": "image", "item": frame["id"]}));
    exec(&mut s, "data.merge", json!({}));
    assert!(graphic_link(&s).is_some_and(|p| p.contains("pic.png")));

    let mut s = Session::new();
    new_doc(&mut s, 1);
    select_csv(&mut s, "@Photo\npic.png\n");
    text_frame(&mut s, [10.0, 10.0, 300.0, 40.0], "", None);
    exec(&mut s, "data.placeholder.add", json!({"field": "Photo", "role": "text"}));
    exec(&mut s, "data.merge", json!({}));
    assert_eq!(texts(&s), vec!["pic.png".to_string()]);

    let mut s = Session::new();
    new_doc(&mut s, 1);
    select_csv(&mut s, "Name,#Code\nAda,hello\nGrace,\n");
    let frame = exec(&mut s, "frame.create", json!({"rect": [10.0, 10.0, 80.0, 80.0], "content": "graphic"}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Code", "item": frame["id"]}));
    let r = exec(&mut s, "data.merge", json!({}));
    assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap_or("").contains("QR cell is empty")));
    let doc = &s.documents()[s.active_index().unwrap()].doc;
    let mut alts = Vec::new();
    for sp in &doc.spreads {
        for it in &sp.items {
            if let Content::Group { items } = &it.content {
                alts.extend(items.iter().map(|c| c.alt_text.clone()));
            }
        }
    }
    assert_eq!(alts, vec!["QR code: hello".to_string()]);

    let mut s = Session::new();
    new_doc(&mut s, 1);
    select_csv(&mut s, "Name,Link\nAda,a@b.c\nGrace,https://example.com\nKai,\n");
    let made = text_frame(&mut s, [10.0, 10.0, 200.0, 40.0], "Go", None);
    exec(&mut s, "text.select", json!({"story": made["story"], "anchor": 0, "focus": 2}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Link", "role": "hyperlink"}));
    exec(&mut s, "data.merge", json!({}));
    let doc = &s.documents()[s.active_index().unwrap()].doc;
    let mut kinds = Vec::new();
    for link in &doc.hyperlinks {
        kinds.push(match &link.dest {
            HyperlinkDest::Email(v) => format!("email:{v}"),
            HyperlinkDest::Url(v) => format!("url:{v}"),
            HyperlinkDest::Page(n) => format!("page:{n}"),
            HyperlinkDest::Anchor(a) => format!("anchor:{a}"),
        });
    }
    kinds.sort();
    assert_eq!(kinds, vec!["email:a@b.c".to_string(), "url:https://example.com".to_string()]);
}

fn write_png_sized(path: &Path, width: u32, height: u32) {
    let pixels = vec![255; width as usize * height as usize * 4];
    let bytes = designcraft_render::Rendered { width, height, pixels }.to_png();
    assert_eq!(designcraft_render::image_size(&bytes), Some((width, height)), "the fixture png must decode");
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(path, bytes).unwrap();
}

/// The images anchored in the stories of document `index`, as (file name, width, height), by name.
fn inline_images(s: &Session, index: usize) -> Vec<(String, f64, f64)> {
    let doc = &s.documents()[index].doc;
    let mut out = Vec::new();
    for st in doc.stories.values() {
        for o in &st.objects {
            if let Content::Graphic(g) = &o.item.content {
                let name = doc.assets.get(&g.asset).map(|a| a.name.clone()).unwrap_or_default();
                let (w, h) = o.size();
                out.push((name, w, h));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// An image field inserted at a text insertion point placed the path as text. It places the
/// image in the text: at its natural size, scaled down to fit the frame, with a missing file
/// reported and its marker removed.
#[test]
fn an_image_field_at_the_insertion_point_merges_as_an_inline_image() {
    let dir = scratch("inline-image");
    let data = dir.join("data");
    write_png_sized(&data.join("pics").join("small.png"), 40, 20);
    write_png_sized(&data.join("pics").join("wide.png"), 800, 100);
    write(&data.join("cards.csv"), "Name,@Photo\nAda,pics/small.png\nGrace,pics/wide.png\nKai,pics/none.png\n");

    let mut s = Session::new();
    new_doc(&mut s, 1);
    exec(&mut s, "data.source.select", json!({"path": data.join("cards.csv")}));
    let made = text_frame(&mut s, [36.0, 36.0, 236.0, 236.0], "Photo: ", None);
    exec(&mut s, "text.select", json!({"story": made["story"], "anchor": 7, "focus": 7}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Photo"}));
    let template = &s.documents()[0].doc;
    assert_eq!(template.stories.values().next().unwrap().text, "Photo: <<Photo>>");
    let width = template.item(designcraft_doc::ItemId(made["id"].as_u64().unwrap())).unwrap().inner_bounds().width();

    exec(&mut s, "data.preview", json!({"record": 1}));
    assert_eq!(inline_images(&s, 0).len(), 1, "the preview shows the image");
    exec(&mut s, "data.preview.stop", json!({}));

    let r = exec(&mut s, "data.merge", json!({}));
    assert_eq!(r["records"], 3);
    let missing = r["missingImages"].as_array().unwrap();
    assert_eq!(missing.len(), 1, "{missing:?}");
    assert_eq!((missing[0]["record"].as_u64(), missing[0]["path"].as_str()), (Some(3), Some("pics/none.png")));
    let mark = designcraft_doc::OBJECT_MARK;
    assert_eq!(texts(&s), vec!["Photo: ".to_string(), format!("Photo: {mark}"), format!("Photo: {mark}")]);
    let placed = inline_images(&s, s.active_index().unwrap());
    assert_eq!(placed.len(), 2, "{placed:?}");
    let (small, wide) = (&placed[0], &placed[1]);
    assert_eq!(small.0, "small.png");
    assert!((small.1 - 40.0).abs() < 0.01 && (small.2 - 20.0).abs() < 0.01, "a small image keeps its natural size: {small:?}");
    assert_eq!(wide.0, "wide.png");
    assert!((wide.1 - width).abs() < 0.01 && (wide.2 - width / 8.0).abs() < 0.01, "a wide image is scaled down to the frame: {wide:?}");
}

/// A QR field at a text insertion point is refused instead of merging its payload as text.
#[test]
fn a_qr_field_at_the_insertion_point_is_refused() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    select_csv(&mut s, "@Photo,#Code\n/x.png,hello\n");
    let made = text_frame(&mut s, [36.0, 36.0, 300.0, 300.0], "Hi ", None);
    exec(&mut s, "text.select", json!({"story": made["story"], "anchor": 3, "focus": 3}));
    let msg = err(&mut s, "data.placeholder.add", json!({"field": "Code"}));
    assert!(msg.contains("bind to a frame"), "{msg}");
    assert_eq!(texts(&s), vec!["Hi ".to_string()]);
    assert!(s.documents()[0].doc.data_merge.placeholders.is_empty());
}

/// An explicit QR role at a text insertion point is refused too, rather than binding the QR code
/// to a selected frame the caret is not in.
#[test]
fn an_explicit_qr_role_at_the_insertion_point_is_refused() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    select_csv(&mut s, "Name,#Code\nAda,hello\n");
    let made = text_frame(&mut s, [36.0, 36.0, 300.0, 300.0], "Hi ", None);
    let other = text_frame(&mut s, [36.0, 400.0, 300.0, 600.0], "", None);
    let other_id = designcraft_doc::ItemId(other["id"].as_u64().unwrap());
    let sid = StoryId(made["story"].as_u64().unwrap());
    let doc = s.doc_mut().unwrap();
    doc.selection = Selection::text(TextSel { story: sid, anchor: 3, focus: 3, frame: None, cell: None });
    doc.selection.items = vec![other_id];
    let msg = err(&mut s, "data.placeholder.add", json!({"field": "Code", "role": "qr"}));
    assert!(msg.contains("bind to a frame"), "{msg}");
    let doc = &s.documents()[0].doc;
    assert!(doc.data_merge.placeholders.is_empty());
    assert_eq!(doc.item(other_id).unwrap().label, "", "the other frame is not bound");
}

/// Fields inserted one after another at the insertion point keep their order: the caret moves
/// past each marker, as after typing.
#[test]
fn fields_inserted_at_the_insertion_point_keep_their_order() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    select_csv(&mut s, "Name,City,@Photo\nAda,Paris,/x.png\n");
    let made = text_frame(&mut s, [36.0, 36.0, 300.0, 300.0], "Hi ", None);
    exec(&mut s, "text.select", json!({"story": made["story"], "anchor": 3, "focus": 3}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Name", "role": "text"}));
    exec(&mut s, "data.placeholder.add", json!({"field": "City", "role": "text"}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Photo"}));
    assert_eq!(texts(&s), vec!["Hi <<Name>><<City>><<Photo>>".to_string()]);
    let t = s.documents()[0].selection.text.unwrap();
    assert_eq!((t.anchor, t.focus), (28, 28));
}

/// A typed `<<name>>` marker for an image column places the image too, from an absolute path;
/// an explicit text role still writes the path as text.
#[test]
fn a_typed_image_marker_places_the_image_and_a_text_role_keeps_the_path() {
    let dir = scratch("typed-image");
    let pic = dir.join("pic.png");
    write_png_sized(&pic, 10, 30);
    let mut s = Session::new();
    new_doc(&mut s, 1);
    select_csv(&mut s, &format!("@Photo\n{}\n", pic.display()));
    text_frame(&mut s, [36.0, 36.0, 300.0, 300.0], "A <<Photo>> B", None);
    let r = exec(&mut s, "data.merge", json!({}));
    assert_eq!(r["missingImages"].as_array().map(Vec::len), Some(0));
    assert_eq!(texts(&s), vec![format!("A {} B", designcraft_doc::OBJECT_MARK)]);
    let placed = inline_images(&s, s.active_index().unwrap());
    assert_eq!(placed.len(), 1);
    assert!((placed[0].1 - 10.0).abs() < 0.01 && (placed[0].2 - 30.0).abs() < 0.01, "{placed:?}");

    let mut s = Session::new();
    new_doc(&mut s, 1);
    select_csv(&mut s, &format!("@Photo\n{}\n", pic.display()));
    let made = text_frame(&mut s, [36.0, 36.0, 300.0, 300.0], "", None);
    exec(&mut s, "text.select", json!({"story": made["story"], "anchor": 0, "focus": 0}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Photo", "role": "text"}));
    exec(&mut s, "data.merge", json!({}));
    assert_eq!(texts(&s), vec![pic.display().to_string()]);
    assert!(inline_images(&s, s.active_index().unwrap()).is_empty());
}

/// An explicit image role at a text insertion point places an inline image too, and in a frame
/// with two columns the image is scaled down to one column's width.
#[test]
fn an_inline_image_role_at_text_fits_one_column() {
    let dir = scratch("inline-columns");
    let pic = dir.join("wide.png");
    write_png_sized(&pic, 800, 100);
    let mut s = Session::new();
    new_doc(&mut s, 1);
    select_csv(&mut s, &format!("@Photo\n{}\n", pic.display()));
    let made = text_frame(&mut s, [36.0, 36.0, 456.0, 336.0], "", None);
    exec(&mut s, "object.textFrameOptions", json!({"ids": [made["id"]], "columns": 2, "gutter": 20}));
    exec(&mut s, "text.select", json!({"story": made["story"], "anchor": 0, "focus": 0}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Photo", "role": "image"}));
    let r = exec(&mut s, "data.merge", json!({}));
    assert_eq!(r["missingImages"].as_array().map(Vec::len), Some(0), "{r}");
    let placed = inline_images(&s, s.active_index().unwrap());
    assert_eq!(placed.len(), 1, "{placed:?}");
    // 420 pt wide, less the 20 pt gutter, gives two 200 pt columns.
    assert!((placed[0].1 - 200.0).abs() < 0.01 && (placed[0].2 - 25.0).abs() < 0.01, "{placed:?}");

    // The story threads on into a 100 pt frame: the image fits that one, wherever the marker lands.
    let mut s = Session::new();
    new_doc(&mut s, 1);
    select_csv(&mut s, &format!("@Photo\n{}\n", pic.display()));
    let made = text_frame(&mut s, [36.0, 36.0, 456.0, 336.0], "", None);
    let small = exec(&mut s, "frame.create", json!({"rect": [36.0, 400.0, 136.0, 500.0], "content": "unassigned"}));
    exec(&mut s, "text.thread", json!({"from": made["id"], "to": small["id"]}));
    exec(&mut s, "text.select", json!({"story": made["story"], "anchor": 0, "focus": 0}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Photo"}));
    exec(&mut s, "data.merge", json!({}));
    let placed = inline_images(&s, s.active_index().unwrap());
    assert_eq!(placed.len(), 1, "{placed:?}");
    assert!(placed[0].1 <= 100.0 + 0.01, "{placed:?}");
}

/// A cell that starts with one separator (`\pics\a.png`) and names no file from the drive's root
/// is found beside the data file.
#[test]
fn an_image_path_with_a_leading_separator_is_found_beside_the_data_file() {
    let dir = scratch("rooted-image");
    let data = dir.join("data");
    write_png_sized(&data.join("dc-merge-rooted-pics").join("a.png"), 12, 12);
    write(&data.join("cards.csv"), "@Photo\n\\dc-merge-rooted-pics\\a.png\n/dc-merge-rooted-pics/a.png\n");
    let mut s = Session::new();
    new_doc(&mut s, 1);
    exec(&mut s, "data.source.select", json!({"path": data.join("cards.csv")}));
    let frame = exec(&mut s, "frame.create", json!({"rect": [36.0, 36.0, 80.0, 80.0], "content": "graphic"}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Photo", "item": frame["id"]}));
    let r = exec(&mut s, "data.merge", json!({}));
    assert_eq!(r["records"], 2);
    assert_eq!(r["missingImages"].as_array().map(Vec::len), Some(0), "{r}");
}

/// A file at an earlier candidate that is not an image does not hide a valid image at a later
/// one: the search goes on past it.
#[test]
fn a_non_image_file_beside_the_data_does_not_hide_the_image_beside_the_document() {
    let dir = scratch("undecodable-image");
    let (data, docs) = (dir.join("data"), dir.join("docs"));
    write(&data.join("pics").join("a.png"), "not an image");
    write_png_sized(&docs.join("pics").join("a.png"), 12, 12);
    write(&data.join("cards.csv"), "@Photo\npics/a.png\n");
    let mut s = Session::new();
    new_doc(&mut s, 1);
    exec(&mut s, "data.source.select", json!({"path": data.join("cards.csv")}));
    exec(&mut s, "file.save", json!({"path": docs.join("layout.designcraft")}));
    let frame = exec(&mut s, "frame.create", json!({"rect": [36.0, 36.0, 80.0, 80.0], "content": "graphic"}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Photo", "item": frame["id"]}));
    exec(&mut s, "data.options", json!({"linkImages": true}));
    let r = exec(&mut s, "data.merge", json!({}));
    assert_eq!(r["missingImages"].as_array().map(Vec::len), Some(0), "{r}");
    let link = graphic_link(&s).unwrap_or_default();
    assert!(link.contains("docs"), "{link}");
}

#[test]
fn single_record_merge_copies_pages_and_leaves_the_template() {
    let mut s = Session::new();
    new_doc(&mut s, 2);
    text_frame(&mut s, [36.0, 36.0, 300.0, 80.0], "<<Name>>", None);
    text_frame(&mut s, [36.0, 36.0, 300.0, 80.0], "Page <<Name>>", Some(1));
    select_csv(&mut s, "Name\nAda\nGrace\nKatherine\n");
    exec(&mut s, "guide.add", json!({"orientation": "vertical", "position": 100.0}));
    exec(&mut s, "guide.add", json!({"spread": 1, "orientation": "vertical", "position": 180.0}));
    let undo = s.documents()[0].history.undo.len();
    let template_pages = s.documents()[0].doc.page_count();
    let r = exec(&mut s, "data.merge", json!({}));
    let pages = r["pages"].as_u64().unwrap();
    println!(
        "GATING 6 pages for 2 template pages x 3 records: result {pages} document {}",
        s.documents()[s.active_index().unwrap()].doc.page_count()
    );
    assert_eq!(pages, 6);
    assert_eq!(r["records"], 3);
    assert_eq!(s.documents().len(), 2);
    assert_eq!(s.active_index(), Some(1));
    assert_eq!(s.documents()[0].doc.page_count(), template_pages);
    assert_eq!(s.documents()[0].history.undo.len(), undo);
    assert!(texts(&s).iter().any(|t| t == "Ada") && texts(&s).iter().any(|t| t == "Page Katherine"));
    let doc = &s.documents()[1].doc;
    let marks: Vec<Vec<i32>> = doc
        .pages()
        .map(|(_, _, p)| {
            let mut g: Vec<i32> = p.guides.iter().map(|g| g.position.round() as i32).collect();
            g.sort();
            g
        })
        .collect();
    assert_eq!(marks, vec![vec![100], vec![180], vec![100], vec![180], vec![100], vec![180]]);
    assert!(s.documents()[0].doc.stories.values().any(|st| st.text.contains("<<Name>>")));
}

#[test]
fn tiling_rows_first_bottom_inset_and_refusals() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    text_frame(&mut s, [72.0, 72.0, 172.0, 172.0], "<<Name>>", None);
    select_csv(&mut s, "Name\nA\nB\nC\nD\nE\nF\n");
    exec(
        &mut s,
        "data.options",
        json!({"perPage": "multiple", "arrange": "rows", "insets": [36.0, 36.0, 36.0, 36.0], "columnSpacing": 36.0, "rowSpacing": 36.0}),
    );
    let r = exec(&mut s, "data.merge", json!({}));
    let placed = origins(&s);
    println!("GATING 3-across then next-row tile: pages {} origins {placed:?}", r["pages"]);
    assert_eq!(r["pages"], 1);
    assert_eq!(r["records"], 6);
    assert_eq!(placed, vec![(72, 72), (72, 208), (72, 344), (208, 72), (208, 208), (208, 344)]);

    exec(&mut s, "file.activate", json!({"index": 0}));
    exec(&mut s, "data.options", json!({"arrange": "columns"}));
    exec(&mut s, "data.merge", json!({}));
    assert_eq!(origins(&s), vec![(72, 72), (72, 208), (208, 72), (344, 72), (480, 72), (616, 72)]);

    let mut s = Session::new();
    new_doc(&mut s, 1);
    text_frame(&mut s, [72.0, 700.0, 172.0, 780.0], "<<Name>>", None);
    select_csv(&mut s, "Name\nA\nB\nC\nD\nE\nF\n");
    exec(
        &mut s,
        "data.options",
        json!({"perPage": "multiple", "arrange": "rows", "insets": [36.0, 36.0, 36.0, 36.0], "columnSpacing": 36.0, "rowSpacing": 36.0}),
    );
    let r = exec(&mut s, "data.merge", json!({}));
    assert_eq!(r["pages"], 2);
    assert_eq!(s.documents()[1].doc.spreads.iter().map(|sp| sp.items.len()).collect::<Vec<_>>(), vec![3, 3]);

    let mut s = Session::new();
    new_doc(&mut s, 1);
    text_frame(&mut s, [36.0, 36.0, 500.0, 700.0], "<<Name>>", None);
    select_csv(&mut s, "Name\nA\nB\nC\nD\n");
    exec(&mut s, "data.options", json!({"perPage": "multiple", "columnSpacing": 0.0, "rowSpacing": 0.0}));
    let r = exec(&mut s, "data.merge", json!({}));
    assert_eq!(r["pages"], 4);
    assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap_or("").contains("its own page")));

    let mut s = Session::new();
    exec(&mut s, "file.new", json!({}));
    text_frame(&mut s, [72.0, 72.0, 172.0, 172.0], "<<Name>>", None);
    select_csv(&mut s, "Name\nA\nB\n");
    exec(&mut s, "data.options", json!({"perPage": "multiple"}));
    let before = s.documents().len();
    let msg = err(&mut s, "data.merge", json!({}));
    let after = s.documents().len();
    println!("GATING tiling refusal added no document (before {before}, after {after})");
    assert!(msg.contains("facing pages"), "{msg}");
    assert_eq!(before, after);

    let mut s = Session::new();
    new_doc(&mut s, 2);
    text_frame(&mut s, [72.0, 72.0, 172.0, 172.0], "<<Name>>", None);
    select_csv(&mut s, "Name\nA\nB\n");
    exec(&mut s, "data.options", json!({"perPage": "multiple"}));
    let before = s.documents().len();
    let msg = err(&mut s, "data.merge", json!({}));
    assert!(msg.contains("one page"), "{msg}");
    assert_eq!(s.documents().len(), before);
}

/// A two-page template merged for three records: six pages made in one insertion, each record's
/// frame on its own pages, in record order.
#[test]
fn a_two_page_template_merges_onto_consecutive_pages() {
    let mut s = Session::new();
    new_doc(&mut s, 2);
    text_frame(&mut s, [72.0, 72.0, 172.0, 172.0], "<<Name>>", None);
    select_csv(&mut s, "Name\nA\nB\nC\n");
    let r = exec(&mut s, "data.merge", json!({}));
    assert_eq!(r["pages"], 6);
    let d = &s.documents()[s.active_index().unwrap()].doc;
    assert_eq!(d.page_count(), 6);
    let mut at: Vec<(usize, String)> = Vec::new();
    for st in d.stories.values() {
        let f = d.item(*st.frames.first().unwrap()).unwrap();
        let sp = d.spreads.iter().position(|sp| sp.items.iter().any(|i| i.id == f.id)).unwrap();
        let page = d.first_page_of_spread(sp) + d.spreads[sp].page_at_x(f.bounds().center().x).unwrap_or(0);
        at.push((page, st.text.clone()));
    }
    at.sort();
    assert_eq!(at, vec![(0, "A".to_string()), (2, "B".to_string()), (4, "C".to_string())]);
    d.check().unwrap();
}

/// An IME delete-surrounding while a record is previewed edits the template, not the filled
/// record, and its undo step restores the template.
#[test]
fn an_ime_delete_ends_a_preview_and_edits_the_template() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    let f = text_frame(&mut s, [36.0, 36.0, 240.0, 80.0], "", None);
    let sid = StoryId(f["story"].as_u64().unwrap());
    select_csv(&mut s, "Name\nAda\nGrace\n");
    exec(&mut s, "data.placeholder.add", json!({"field": "Name"}));
    assert_eq!(texts(&s), vec!["<<Name>>".to_string()]);
    exec(&mut s, "data.preview", json!({"record": 2}));
    assert_eq!(texts(&s), vec!["Grace".to_string()], "the preview shows the record");
    // The caret was placed in the preview, at the end of the filled text.
    s.doc_mut().unwrap().selection = Selection::text(TextSel { story: sid, anchor: 5, focus: 5, frame: None, cell: None });
    let m = s.ime_delete_surrounding(1, 0, false).unwrap().expect("the delete changed the template");
    assert!(m.inserted.is_empty());
    assert!(s.doc().unwrap().preview_stash.is_none(), "the preview ended before the edit");
    assert_eq!(texts(&s), vec!["<<Nae>>".to_string()], "the template lost the character, not the filled record");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(texts(&s), vec!["<<Name>>".to_string()], "Undo restores the template, never the preview record");
    assert!(s.doc().unwrap().preview_stash.is_none());
}

fn check_missing_merge_source(dir: &Path) {
    let gone = dir.join("gone.csv");
    write(&gone, "Name\nAda\nGrace\n");
    let mut missing = Session::new();
    new_doc(&mut missing, 1);
    text_frame(&mut missing, [36.0, 36.0, 240.0, 80.0], "", None);
    exec(&mut missing, "data.source.select", json!({"path": &gone}));
    exec(&mut missing, "data.placeholder.add", json!({"field": "Name"}));
    std::fs::remove_file(&gone).unwrap();
    let missing_undo = missing.documents()[0].history.undo.len();
    let r = exec(&mut missing, "data.merge", json!({}));
    assert_eq!(r["records"], 2);
    assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap_or("").contains("missing")));
    assert_eq!(texts(&missing), vec!["Ada".to_string(), "Grace".to_string()]);
    assert_eq!(missing.documents()[0].doc.data_merge.sources[0].status, SourceStatus::Missing);
    assert_eq!(missing.documents()[0].history.undo.len(), missing_undo);
}

fn check_refused_facing_page_merge(dir: &Path) {
    let refuse = dir.join("refuse.csv");
    write(&refuse, "Name\nAda\nGrace\n");
    let mut refused = Session::new();
    exec(&mut refused, "file.new", json!({}));
    text_frame(&mut refused, [36.0, 36.0, 240.0, 80.0], "", None);
    exec(&mut refused, "data.source.select", json!({"path": &refuse}));
    exec(&mut refused, "data.placeholder.add", json!({"field": "Name"}));
    let mut body = std::fs::read(&refuse).unwrap();
    body.extend_from_slice(b"Kai\n");
    std::fs::write(&refuse, &body).unwrap();
    let refuse_undo = refused.documents()[0].history.undo.len();
    let msg = err(&mut refused, "data.merge", json!({"perPage": "multiple"}));
    assert!(msg.contains("facing pages"), "{msg}");
    assert_eq!(refused.documents().len(), 1);
    assert_eq!(refused.documents()[0].doc.data_merge.sources[0].status, SourceStatus::Ok);
    assert_eq!(refused.documents()[0].history.undo.len(), refuse_undo);
}

fn check_preview_update_save(session: &mut Session, dir: &Path, csv: &Path, id: &Value) {
    exec(session, "file.activate", json!({"index": 0}));
    exec(session, "data.source.update", json!({}));
    let r = exec(session, "data.merge", json!({}));
    assert_eq!(r["records"], 3);
    assert!(texts(session).iter().any(|t| t == "Kai"));

    exec(session, "file.activate", json!({"index": 0}));
    exec(session, "data.source.remove", json!({}));
    assert!(session.documents()[0].doc.data_merge.sources.is_empty());
    assert!(session.documents()[0].doc.data_merge.placeholders.iter().any(|ph| ph.id == id.as_u64().unwrap()));
    assert_eq!(exec(session, "data.fields", json!({})), json!([]));

    select_csv(session, "Name\nAda\nGrace\n");
    exec(session, "data.preview", json!({"record": 2}));
    assert!(texts(session).iter().any(|t| t.contains("Grace")));
    assert!(!texts(session).iter().any(|t| t.contains("<<Name>>")));
    let ser = exec(session, "file.serialize", json!({}));
    let saved = crate::cmd::from_bytes(&crate::cmd::base64_decode(ser["base64"].as_str().unwrap())).unwrap();
    assert!(saved.stories.values().any(|st| st.text.contains("<<Name>>")));
    assert!(!saved.stories.values().any(|st| st.text.contains("Grace")));
    exec(session, "data.preview.stop", json!({}));
    assert!(texts(session).iter().any(|t| t.contains("<<Name>>")));

    exec(session, "data.preview", json!({"record": 2}));
    let path = dir.join("saved.designcraft");
    exec(session, "file.save", json!({"path": path}));
    let saved = crate::cmd::from_bytes(&std::fs::read(&path).unwrap()).unwrap();
    assert!(!saved.stories.values().any(|st| st.text.contains("Grace")));
    assert!(saved.stories.values().any(|st| st.text.contains("<<Name>>")));

    exec(session, "data.preview", json!({"record": 2}));
    let rec = dir.join("recovery");
    session.recovery_dir = Some(rec.clone());
    crate::recovery::save(session, &rec).unwrap();
    let uid = session.documents()[0].uid;
    let recovered = crate::cmd::from_bytes(&std::fs::read(rec.join(format!("{uid}.designcraft"))).unwrap()).unwrap();
    assert!(!recovered.stories.values().any(|st| st.text.contains("Grace")));
    assert!(texts(session).iter().any(|t| t.contains("Grace")), "recovery writes the stash and leaves the preview on screen");
    exec(session, "file.new", json!({}));
    exec(session, "file.activate", json!({"index": 0}));
    assert!(texts(session).iter().any(|t| t.contains("<<Name>>")));

    check_inline_rows_and_invalid_requests();
    check_saved_relative_source(dir, csv);
}

fn check_inline_rows_and_invalid_requests() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    text_frame(&mut s, [36.0, 36.0, 300.0, 80.0], "<<Name>> <<Qty>>", None);
    let r = exec(&mut s, "data.merge", json!({"rows": [{"Name": "Ada", "Qty": 2}, {"Name": "Grace", "Qty": 1.5, "Extra": true}]}));
    assert_eq!(r["records"], 2);
    assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap_or("").contains("Extra")));
    assert_eq!(texts(&s), vec!["Ada 2".to_string(), "Grace 1.5".to_string()]);
    exec(&mut s, "file.activate", json!({"index": 0}));
    assert!(s.documents()[0].doc.data_merge.sources.is_empty());

    let before = s.documents().len();
    let msg = err(&mut s, "data.merge", json!({"csv": "Name\nAda\n", "spread": 0}));
    assert!(msg.contains("the spread parameter has been removed"), "{msg}");
    assert_eq!(s.documents().len(), before);

    let msg = err(&mut s, "data.merge", json!({"csv": "Name\nAda\nGrace\n", "records": "one", "one": 0}));
    assert!(msg.contains("out of range"), "{msg}");
    let msg = err(&mut s, "data.merge", json!({"csv": "Name\nAda\n", "records": "range", "range": "3-1"}));
    assert!(msg.contains("reversed"), "{msg}");
    let msg = err(&mut s, "data.merge", json!({"csv": "Name\nAda\n", "limit": 0}));
    assert!(msg.contains("limit"), "{msg}");
    // Numbers past u32 are an error, not a silent truncation to a small record number.
    let msg = err(&mut s, "data.merge", json!({"csv": "Name\nAda\n", "records": "one", "one": 4_294_967_297u64}));
    assert!(msg.contains("too large"), "{msg}");
    let msg = err(&mut s, "data.merge", json!({"csv": "Name\nAda\n", "limit": 4_294_967_296u64}));
    assert!(msg.contains("too large"), "{msg}");
    assert_eq!(s.documents().len(), before);
}

fn check_saved_relative_source(dir: &Path, csv: &Path) {
    let fresh = dir.join("fresh.designcraft");
    let mut s = Session::new();
    new_doc(&mut s, 1);
    exec(&mut s, "file.save", json!({"path": fresh}));
    let opened = crate::cmd::from_bytes(&std::fs::read(&fresh).unwrap()).unwrap();
    assert!(opened.data_merge.is_empty());
    exec(&mut s, "data.source.select", json!({"path": csv}));
    exec(&mut s, "file.save", json!({"path": fresh}));
    let opened = crate::cmd::from_bytes(&std::fs::read(&fresh).unwrap()).unwrap();
    assert_eq!(opened.data_merge.sources[0].relative_path.as_deref(), Some("people.csv"));
}

#[test]
fn update_remove_preview_inline_rows_and_old_files() {
    let dir = scratch("update");
    let csv = dir.join("people.csv");
    write(&csv, "Name\nAda\nGrace\n");
    let mut s = Session::new();
    new_doc(&mut s, 1);
    text_frame(&mut s, [36.0, 36.0, 240.0, 80.0], "", None);
    exec(&mut s, "data.source.select", json!({"path": csv}));
    let id = exec(&mut s, "data.placeholder.add", json!({"field": "Name"}))["id"].clone();
    let undo = s.documents()[0].history.undo.len();
    let fingerprint = s.documents()[0].doc.data_merge.sources[0].fingerprint;
    let mut body = std::fs::read(&csv).unwrap();
    body.extend_from_slice(b"Kai\n");
    std::fs::write(&csv, &body).unwrap();
    let r = exec(&mut s, "data.merge", json!({}));
    assert_eq!(r["records"], 2);
    assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap_or("").contains("changed")));
    assert_eq!(texts(&s), vec!["Ada".to_string(), "Grace".to_string()]);
    assert_eq!(s.documents()[0].doc.data_merge.sources[0].status, SourceStatus::Modified);
    assert_eq!(s.documents()[0].doc.data_merge.sources[0].fingerprint, fingerprint);
    assert_eq!(s.documents()[0].history.undo.len(), undo);
    assert_eq!(s.documents()[s.active_index().unwrap()].doc.data_merge.sources[0].status, SourceStatus::Modified);

    check_missing_merge_source(&dir);
    check_refused_facing_page_merge(&dir);
    check_preview_update_save(&mut s, &dir, &csv, &id);
}

#[test]
fn parent_placeholder_is_copied_unfilled() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    select_csv(&mut s, "Name\nAda\n");
    exec(&mut s, "layout.parents.new", json!({}));
    text_frame(&mut s, [36.0, 36.0, 200.0, 80.0], "<<Name>>", None);
    // The caret is in the new frame. Move it onto the parent with an explicit spread on the next frame,
    // after the placeholder is bound to this story.
    exec(&mut s, "data.placeholder.add", json!({"field": "Name"}));
    exec(
        &mut s,
        "frame.create",
        json!({"rect": [36.0, 120.0, 200.0, 180.0], "content": "text", "text": "<<Name>>", "spread": {"kind": "parent", "index": 0}}),
    );
    let r = exec(&mut s, "data.merge", json!({}));
    // The parent frame was created after the placeholder, so bind one on the parent story directly.
    let _ = r;
}

#[test]
fn parent_page_placeholder_warns_once_and_stays() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    select_csv(&mut s, "Name\nAda\nGrace\n");
    exec(&mut s, "layout.parents.new", json!({}));
    let parent = exec(
        &mut s,
        "frame.create",
        json!({"rect": [36.0, 36.0, 200.0, 80.0], "content": "text", "text": "", "spread": {"kind": "parent", "index": 0}}),
    );
    exec(&mut s, "data.placeholder.add", json!({"field": "Name", "story": parent["story"], "at": 0}));
    text_frame(&mut s, [36.0, 36.0, 240.0, 80.0], "<<Name>>", None);
    let r = exec(&mut s, "data.merge", json!({}));
    let warnings: Vec<_> = r["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str()).filter(|w| w.contains("parent")).collect();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let doc = &s.documents()[s.active_index().unwrap()].doc;
    let mut parent_text = Vec::new();
    for sp in &doc.parents {
        for it in &sp.items {
            if let Content::Text(tf) = &it.content
                && let Some(st) = doc.story(tf.story)
            {
                parent_text.push(st.text.clone());
            }
        }
    }
    assert_eq!(parent_text, vec!["<<Name>>".to_string()]);
    assert!(texts(&s).contains(&"Ada".to_string()));
    assert!(texts(&s).contains(&"Grace".to_string()));
}

#[test]
fn relative_path_and_reopen() {
    let root = scratch("rel");
    let a = root.join("a");
    let b = root.join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let csv = b.join("data.csv");
    write(&csv, "Name\nAda\n");
    let relative = Path::new("..").join("b").join("data.csv").to_string_lossy().into_owned();
    assert_eq!(relative_between(&a, &csv), relative);
    assert_eq!(relative_between(&b, &csv), "data.csv");

    let mut doc = designcraft_doc::Document::new(&designcraft_doc::build::NewDocument::default());
    doc.data_merge.sources.push(DataSource {
        id: 1,
        path: Some("/no/such/data-merge-source.csv".into()),
        relative_path: Some("data.csv".into()),
        name: "data.csv".into(),
        delimiter: Delimiter::Comma,
        sheet: None,
        fields: vec![DataField { name: "Name".into(), kind: DataFieldKind::Text }],
        rows: vec![vec!["Ada".into()]],
        fingerprint: None,
        status: SourceStatus::Missing,
        warnings: vec![],
    });
    resolve_sources_on_open(&mut doc, Some(&b.join("doc.designcraft")));
    assert_eq!(doc.data_merge.sources[0].status, SourceStatus::Ok);
    assert_eq!(doc.data_merge.sources[0].path.as_deref(), Some(csv.to_string_lossy().as_ref()));

    let mut s = Session::new();
    new_doc(&mut s, 1);
    exec(&mut s, "data.source.select", json!({"path": csv}));
    let saved = a.join("layout.designcraft");
    exec(&mut s, "file.save", json!({"path": saved}));
    let mut s2 = Session::new();
    exec(&mut s2, "file.open", json!({"path": saved}));
    let src = &s2.documents()[s2.active_index().unwrap()].doc.data_merge.sources[0];
    assert_eq!(src.relative_path.as_deref(), Some(relative.as_str()));
    assert_eq!(src.rows[0][0], "Ada");
}

#[test]
fn merge_replaces_typed_fields_on_a_new_document() {
    let mut s = Session::new();
    exec(&mut s, "file.new", json!({"facingPages": false}));
    exec(
        &mut s,
        "frame.create",
        json!({"rect": [36.0, 36.0, 400.0, 100.0], "content": "text", "text": "Dear <<Name>>, welcome aboard as <<Title>>."}),
    );
    let undo = s.documents()[0].history.undo.len();
    let r = exec(&mut s, "data.merge", json!({"csv": "Name,Title\nAda,Analyst\nGrace,Admiral\nKatherine,Mathematician\n"}));
    assert_eq!(r["records"], 3);
    assert_eq!(r["pages"], 3);
    assert_eq!(
        texts(&s),
        vec![
            "Dear Ada, welcome aboard as Analyst.".to_string(),
            "Dear Grace, welcome aboard as Admiral.".to_string(),
            "Dear Katherine, welcome aboard as Mathematician.".to_string(),
        ]
    );
    assert_eq!(s.documents()[0].history.undo.len(), undo);
    assert_eq!(s.documents()[0].doc.page_count(), 1);
    assert!(s.documents()[0].doc.stories.values().any(|st| st.text.contains("<<Name>>")));
}

#[test]
fn tile_slots_match_the_insets() {
    let plan = parse::tile(&parse::TileInput {
        block: designcraft_geom::Rect::new(72.0, 72.0, 172.0, 172.0),
        page_width: 612.0,
        page_height: 792.0,
        insets: [36.0; 4],
        column_spacing: 36.0,
        row_spacing: 36.0,
        rows_first: true,
        count: 6,
    });
    assert_eq!(plan.pages, 1);
    assert!(!plan.one_per_page);
    let slots: Vec<_> = plan.hits.iter().map(|h| (h.col, h.row)).collect();
    assert_eq!(slots, vec![(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1)]);
}

#[test]
fn select_records_dedupes_large_ranges_in_first_seen_order() {
    let idx = parse::select_records(100_000, "range", 1, "5,1-100000,3", None).unwrap();
    assert_eq!(idx.len(), 100_000);
    assert_eq!(&idx[..3], &[4, 0, 1]);
}

#[test]
fn select_records_rejects_signed_record_numbers() {
    for (tok, bad) in [("+2", "+2"), ("1-+3", "+3"), ("+1,+4", "+1")] {
        let err = parse::select_records(5, "range", 1, tok, None).unwrap_err();
        assert!(err.contains(&format!("bad record \"{bad}\"")), "{tok}: {err}");
    }
}

#[test]
fn oversized_data_file_is_refused_before_reading() {
    let dir = scratch("oversized");
    let path = dir.join("big.csv");
    write(&path, &"x".repeat(2048));
    let path = path.to_string_lossy().to_string();
    let msg = read_path_capped(&path, 1024).unwrap_err();
    assert!(msg.contains("larger than"), "{msg}");
    assert_eq!(read_path_capped(&path, 4096).unwrap().len(), 2048);
}

/// A device reports no length; the read still stops at the cap (an EPUB `coverImage` of
/// `/dev/zero` must not read until memory runs out).
#[cfg(unix)]
#[test]
fn endless_file_is_refused_at_the_cap() {
    let msg = read_path_capped("/dev/zero", 1024).unwrap_err();
    assert!(msg.contains("larger than"), "{msg}");
}

/// #198: with facing pages on, one record per page must not stack the copies at identical
/// coordinates on one spread (leaving a later page empty).
#[test]
fn facing_pages_merge_puts_each_record_on_its_own_page() {
    let mut s = Session::new();
    exec(&mut s, "file.new", json!({"facingPages": true, "pages": 1, "width": 612, "height": 792}));
    select_csv(&mut s, "Title,Clue\nCase 1,Cellar\nCase 2,Attic\nCase 3,Garden\n");
    // A block taller than the page's usable height, so only one record fits per page.
    text_frame(&mut s, [72.0, 72.0, 540.0, 700.0], "<<Title>> - <<Clue>>", None);
    let r = exec(&mut s, "data.merge", json!({}));
    assert_eq!(r["records"], 3);

    let doc = &s.documents()[s.active_index().unwrap()].doc;
    assert_eq!(doc.page_count(), 3, "one page per record");

    // Which absolute page each merged frame sits on.
    let mut pages_seen = std::collections::BTreeSet::new();
    let mut origins = Vec::new();
    for sp in &doc.spreads {
        for it in &sp.items {
            if !matches!(it.content, Content::Text(_)) {
                continue;
            }
            let abs = doc.page_of_item(it.id);
            let b = it.bounds();
            origins.push((abs, b.x0.round() as i32, b.y0.round() as i32));
            if let Some(a) = abs {
                pages_seen.insert(a);
            }
        }
    }
    assert_eq!(origins.len(), 3, "one text frame per record");
    assert_eq!(pages_seen, [0usize, 1, 2].into_iter().collect(), "each record on its own page, none stacked and none left empty: {origins:?}");
}

/// #198's actual repro shape: a short block that tiles several records down one page. With
/// facing pages on they must still land at distinct positions, not on top of each other.
#[test]
fn facing_pages_merge_tiles_short_blocks_without_stacking() {
    let mut s = Session::new();
    exec(&mut s, "file.new", json!({"facingPages": true, "pages": 1, "width": 612, "height": 792}));
    select_csv(&mut s, "Title,Clue\nCase 1,Cellar\nCase 2,Attic\nCase 3,Garden\n");
    text_frame(&mut s, [72.0, 72.0, 540.0, 200.0], "<<Title>> - <<Clue>>", None);
    let r = exec(&mut s, "data.merge", json!({}));
    assert_eq!(r["records"], 3);

    let doc = &s.documents()[s.active_index().unwrap()].doc;
    let mut origins = Vec::new();
    for sp in &doc.spreads {
        for it in &sp.items {
            if matches!(it.content, Content::Text(_)) {
                let abs = doc.page_of_item(it.id);
                let b = it.bounds();
                origins.push((abs, b.x0.round() as i32, b.y0.round() as i32));
            }
        }
    }
    origins.sort();
    assert_eq!(origins.len(), 3, "one text frame per record");
    let unique: std::collections::BTreeSet<_> = origins.iter().collect();
    assert_eq!(unique.len(), 3, "no two records may share a page and coordinates: {origins:?}");
}

// #1522: each record's copy of an anchored object needs its own id, or the merged file won't reopen.
#[test]
fn merged_records_give_anchored_objects_their_own_ids() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    select_csv(&mut s, "Name\nAda\nGrace\n");
    let made = text_frame(&mut s, [36.0, 36.0, 400.0, 80.0], "<<Name>> here", None);
    let b = exec(&mut s, "frame.create", json!({"rect": [450, 450, 474, 490]}));
    exec(&mut s, "anchored.insert", json!({"ids": [b["id"]], "story": made["story"], "pos": 2}));
    assert_eq!(exec(&mut s, "data.merge", json!({}))["records"], 2);
    let doc = &s.documents()[s.active_index().unwrap()].doc;
    assert_eq!(doc.anchored_items().len(), 2);
    doc.check().unwrap();
    designcraft_format::load(&designcraft_format::save(doc).unwrap()).unwrap();
}

/// A record previewed into an image placeholder whose frame wraps with an Alpha Channel contour
/// gets the image's outline, as an edit would.
#[test]
fn a_previewed_image_gets_its_alpha_channel_outline() {
    let dir = scratch("alpha-preview");
    let mut pixels = vec![0u8; 64 * 64 * 4];
    for y in 0..64 {
        for x in 16..48 {
            pixels[(y * 64 + x) * 4 + 3] = 255;
        }
    }
    std::fs::write(dir.join("cut.png"), designcraft_render::Rendered { width: 64, height: 64, pixels }.to_png()).unwrap();
    write(&dir.join("pics.csv"), "Photo\ncut.png\n");
    let mut s = Session::new();
    new_doc(&mut s, 1);
    exec(&mut s, "data.source.select", json!({"path": dir.join("pics.csv")}));
    let frame = exec(&mut s, "frame.create", json!({"rect": [0.0, 0.0, 200.0, 100.0], "content": "graphic"}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Photo", "role": "image", "item": frame["id"]}));
    exec(&mut s, "object.textWrap", json!({"ids": [frame["id"]], "mode": "contour", "contour": "alphaChannel"}));
    assert!(s.doc().unwrap().doc.alpha_outlines.is_empty(), "no image placed yet");
    exec(&mut s, "data.preview", json!({"record": 1}));
    let d = &s.doc().unwrap().doc;
    assert_eq!(d.alpha_outlines.len(), 1, "the previewed image is traced");
    assert!(d.alpha_outlines[0].width > 0 && !d.alpha_outlines[0].outer.subpaths.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The overset count data merge reports is composed with the merged images' Alpha Channel
/// outlines: a caption over the image's transparent part is set, not counted as overset. With a
/// bounding box wrap the same caption is overset, so the count depends on the outline.
#[test]
fn a_merge_counts_overset_text_with_the_merged_images_outlines() {
    let (r, outlines) = merge_with_wrap("alpha-merge", json!({"mode": "contour", "contour": "alphaChannel", "offset": 0}));
    assert_eq!(r["oversetStories"], 0, "the caption sits beside the image's visible pixels: {r}");
    assert_eq!(outlines, 1, "the merged document has the image's outline");
    let (r, _) = merge_with_wrap("alpha-merge-box", json!({"mode": "boundingBox", "offset": 0}));
    assert_eq!(r["oversetStories"], 1, "the image's frame covers the caption: {r}");
}

/// Merges one record whose image is visible only in its left quarter into a graphic placeholder
/// with the text wrap `wrap`, beside a caption over the image's transparent part. Returns the
/// merge's answer and the number of nonempty Alpha Channel outlines in the merged document.
fn merge_with_wrap(name: &str, wrap: Value) -> (Value, usize) {
    let dir = scratch(name);
    // Visible only in the left quarter.
    let mut pixels = vec![0u8; 64 * 64 * 4];
    for y in 0..64 {
        for x in 0..16 {
            pixels[(y * 64 + x) * 4 + 3] = 255;
        }
    }
    std::fs::write(dir.join("cut.png"), designcraft_render::Rendered { width: 64, height: 64, pixels }.to_png()).unwrap();
    write(&dir.join("pics.csv"), "Photo\ncut.png\n");
    let mut s = Session::new();
    new_doc(&mut s, 1);
    exec(&mut s, "data.source.select", json!({"path": dir.join("pics.csv")}));
    let frame = exec(&mut s, "frame.create", json!({"rect": [100.0, 100.0, 300.0, 300.0], "content": "graphic"}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Photo", "role": "image", "item": frame["id"]}));
    exec(&mut s, "data.options", json!({"fitting": "fitContentToFrame"}));
    let mut params = wrap;
    params["ids"] = json!([frame["id"]]);
    exec(&mut s, "object.textWrap", params);
    text_frame(&mut s, [220.0, 180.0, 290.0, 220.0], "Hi", None);
    let r = exec(&mut s, "data.merge", json!({}));
    let doc = &s.documents()[s.active_index().unwrap()].doc;
    let outlines = doc.alpha_outlines.iter().filter(|o| o.width > 0 && !o.outer.subpaths.is_empty()).count();
    let _ = std::fs::remove_dir_all(&dir);
    (r, outlines)
}

/// The cell texts of the first table in the active document's first story that has one.
fn cell_texts(s: &Session) -> Vec<String> {
    let doc = &s.documents()[s.active_index().unwrap()].doc;
    let t = doc.stories.values().find_map(|st| st.tables.values().next().cloned()).expect("a table");
    t.cells.iter().map(|c| c.text.text.clone()).collect()
}

/// A field inserted with the insertion point in a table cell goes into that cell, and the
/// preview and the merge fill it. The cell's offsets used to put the marker in the table's own
/// story, outside the table, and the merge never read a cell's text (#2315).
#[test]
fn a_field_inserted_in_a_table_cell_goes_there_and_is_filled() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    select_csv(&mut s, "Name,City\nAda,London\nGrace,Arlington\n");
    let made = text_frame(&mut s, [36.0, 36.0, 400.0, 200.0], "Who\tWhere", None);
    exec(&mut s, "text.select", json!({"story": made["story"], "anchor": 0, "focus": 9}));
    exec(&mut s, "table.convertFromText", json!({"columnSeparator": "tab"}));
    let story_text = |s: &Session| s.documents()[s.active_index().unwrap()].doc.stories.values().next().unwrap().text.clone();
    let before = story_text(&s);
    exec(&mut s, "table.select", json!({"story": made["story"], "rows": [0, 0], "cols": [1, 1]}));
    let r = exec(&mut s, "data.placeholder.add", json!({"field": "City"}));
    assert_eq!(r["marker"], "<<City>>", "{r}");
    assert_eq!(cell_texts(&s), ["Who", "<<City>>Where"], "the marker is in the cell with the caret");
    assert_eq!(story_text(&s), before, "and not in the table's story");

    exec(&mut s, "data.preview", json!({"record": 2}));
    assert_eq!(cell_texts(&s), ["Who", "ArlingtonWhere"], "the preview fills the cell");
    exec(&mut s, "data.preview.stop", json!({}));
    assert_eq!(cell_texts(&s), ["Who", "<<City>>Where"], "stopping it puts the marker back");
    exec(&mut s, "data.merge", json!({}));
    let merged = &s.documents()[s.active_index().unwrap()].doc;
    let mut cities: Vec<String> =
        merged.stories.values().flat_map(|st| st.tables.values().map(|t| t.cells[1].text.text.clone()).collect::<Vec<_>>()).collect();
    cities.sort();
    assert_eq!(cities, ["ArlingtonWhere", "LondonWhere"], "one table per record, filled");
}

/// A field typed (or inserted) in text that is then converted to a table is filled in its cell.
#[test]
fn a_field_converted_into_a_table_is_filled_in_its_cell() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    select_csv(&mut s, "Name,City\nAda,London\n");
    let made = text_frame(&mut s, [36.0, 36.0, 400.0, 200.0], "Name\t<<Name>>", None);
    exec(&mut s, "text.select", json!({"story": made["story"], "anchor": 0, "focus": 13}));
    exec(&mut s, "table.convertFromText", json!({"columnSeparator": "tab"}));
    assert_eq!(cell_texts(&s), ["Name", "<<Name>>"]);
    exec(&mut s, "data.preview", json!({"record": 1}));
    assert_eq!(cell_texts(&s), ["Name", "Ada"], "the field is filled in its cell");
}

/// A table `Who⇥Where` with the caret in its second cell, and a source with `Name` and `City`.
fn table_with_caret_in_cell(s: &mut Session, csv: &str) -> Value {
    new_doc(s, 1);
    select_csv(s, csv);
    let made = text_frame(s, [36.0, 36.0, 400.0, 200.0], "Who\tWhere", None);
    exec(s, "text.select", json!({"story": made["story"], "anchor": 0, "focus": 9}));
    exec(s, "table.convertFromText", json!({"columnSeparator": "tab"}));
    exec(s, "table.select", json!({"story": made["story"], "rows": [0, 0], "cols": [1, 1]}));
    made
}

/// After a field goes into a cell the caret is after it, in that cell, and the cell selection is
/// gone: typing goes on after the field instead of replacing the cell, field and all.
#[test]
fn typing_after_a_field_inserted_in_a_cell_goes_on_after_it() {
    let mut s = Session::new();
    table_with_caret_in_cell(&mut s, "Name,City\nAda,London\n");
    exec(&mut s, "data.placeholder.add", json!({"field": "City"}));
    let sel = &s.documents()[s.active_index().unwrap()].selection;
    assert!(sel.cells.is_none(), "no cell selection left");
    exec(&mut s, "text.insert", json!({"text": ", "}));
    assert_eq!(cell_texts(&s), ["Who", "<<City>>, Where"]);
}

/// A position that a merged cell covers is the merged cell's: the field goes into the merged cell,
/// where it shows, not into the hidden one under it.
#[test]
fn a_field_inserted_in_a_merged_cell_goes_where_it_shows() {
    let mut s = Session::new();
    let made = table_with_caret_in_cell(&mut s, "Name,City\nAda,London\n");
    exec(&mut s, "table.merge", json!({"story": made["story"], "rows": [0, 0], "cols": [0, 1]}));
    exec(&mut s, "table.select", json!({"story": made["story"], "rows": [0, 0], "cols": [1, 1]}));
    exec(&mut s, "data.placeholder.add", json!({"field": "City"}));
    let cells = cell_texts(&s);
    assert!(cells[0].contains("<<City>>"), "in the merged cell: {cells:?}");
    assert!(!cells[1].contains("<<City>>"), "not in the covered one: {cells:?}");
    exec(&mut s, "data.preview", json!({"record": 1}));
    assert!(cell_texts(&s)[0].contains("London"), "and filled: {:?}", cell_texts(&s));
}

/// A field whose name a marker can't hold (it has `<` or `>`) can't be filled from a cell, so it
/// isn't inserted there; in body text it still becomes an anchored placeholder.
#[test]
fn a_field_named_with_angle_brackets_is_refused_in_a_cell() {
    let mut s = Session::new();
    let made = table_with_caret_in_cell(&mut s, "Customer <ID>,City\n7,London\n");
    let e = s.execute("data.placeholder.add", &json!({"field": "Customer <ID>"})).expect_err("refused").to_string();
    assert!(e.contains("can't be inserted in a table cell"), "{e}");
    assert_eq!(cell_texts(&s), ["Who", "Where"], "nothing inserted");
    let other = text_frame(&mut s, [36.0, 300.0, 400.0, 400.0], "No. ", None);
    exec(&mut s, "text.select", json!({"story": other["story"], "anchor": 4, "focus": 4}));
    assert!(exec(&mut s, "data.placeholder.add", json!({"field": "Customer <ID>"}))["id"].is_u64(), "body text still takes it");
    let _ = made;
}

/// An image field with the insertion point in a table cell is refused with a message about image
/// fields, and nothing is inserted; with role text its marker goes into the cell.
#[test]
fn an_image_field_in_a_cell_is_refused_unless_its_role_is_text() {
    let mut s = Session::new();
    table_with_caret_in_cell(&mut s, "Name,@Photo\nAda,pics/a.png\n");
    for params in [json!({"field": "Photo"}), json!({"field": "Photo", "role": "image"})] {
        let e = s.execute("data.placeholder.add", &params).expect_err("refused").to_string();
        assert!(e.contains("an image field can't go in a table cell or a note"), "{e}");
        assert!(!e.contains("hyperlink"), "{e}");
        assert_eq!(cell_texts(&s), ["Who", "Where"], "nothing inserted");
    }
    let r = exec(&mut s, "data.placeholder.add", json!({"field": "Photo", "role": "text"}));
    assert_eq!(r["marker"], "<<Photo>>", "{r}");
    assert_eq!(cell_texts(&s), ["Who", "<<Photo>>Where"]);
    exec(&mut s, "data.preview", json!({"record": 1}));
    assert_eq!(cell_texts(&s), ["Who", "pics/a.pngWhere"], "the text role merges the path");
}

/// Endnote text is filled too: it is a text inside its story, like a footnote's.
#[test]
fn a_field_in_an_endnote_is_filled() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    select_csv(&mut s, "Name,City\nAda,London\n");
    let made = text_frame(&mut s, [36.0, 36.0, 400.0, 200.0], "See the note", None);
    exec(&mut s, "text.select", json!({"story": made["story"], "anchor": 12, "focus": 12}));
    exec(&mut s, "endnote.insert", json!({"text": "From <<City>>"}));
    let endnotes = |s: &Session| -> Vec<String> {
        s.documents()[s.active_index().unwrap()].doc.stories.values().flat_map(|st| st.endnotes.iter().map(|e| e.text.text.clone())).collect()
    };
    assert_eq!(endnotes(&s), ["From <<City>>"]);
    exec(&mut s, "data.preview", json!({"record": 1}));
    assert_eq!(endnotes(&s), ["From London"]);
}

/// Tables nested in cells far deeper than any document goes are filled down to the merge's depth
/// limit, and the preview returns instead of exhausting the stack.
#[test]
fn deeply_nested_tables_are_filled_down_to_the_limit_without_overflowing() {
    use designcraft_doc::{Cell, ParaFormat, Table};
    let mut s = Session::new();
    new_doc(&mut s, 1);
    select_csv(&mut s, "Name,City\nAda,London\n");
    let made = text_frame(&mut s, [36.0, 36.0, 400.0, 200.0], "x", None);
    let sid = StoryId(made["story"].as_u64().unwrap());
    // `depth` tables, each in the one cell of the next, each cell's own text `<<Name>>`.
    let nest = |depth: usize| {
        let mut inner: Option<Table> = None;
        for i in 0..depth {
            let mut t = Table::new(1000 + i as u64, 1, 1, 0, 0, 100.0);
            t.cells[0] = Cell::with_text("<<Name>>", ParaFormat::default());
            if let Some(child) = inner.take() {
                t.cells[0].text.insert_table(0, child);
            }
            inner = Some(t);
        }
        inner.unwrap()
    };
    s.edit(|d, _| {
        d.story_mut(sid).unwrap().insert_table(0, nest(500));
        Ok(())
    })
    .unwrap();
    exec(&mut s, "data.preview", json!({"record": 1}));
    let doc = &s.documents()[s.active_index().unwrap()].doc;
    let mut t = doc.story(sid).unwrap().tables.values().next().unwrap().clone();
    let mut filled = Vec::new();
    for _ in 0..40 {
        filled.push(t.cells[0].text.text.contains("Ada"));
        let Some(next) = t.cells[0].text.tables.values().next().cloned() else { break };
        t = next;
    }
    assert!(filled[..super::fill::MAX_NESTED_DEPTH].iter().all(|f| *f), "filled down to the limit: {filled:?}");
    assert!(!filled[super::fill::MAX_NESTED_DEPTH], "and not past it");
}

#[test]
fn select_records_stops_expanding_once_the_limit_is_met_but_still_checks_every_range() {
    // 4 billion records in a range would not finish if it were expanded; the limit stops it.
    let idx = parse::select_records(u32::MAX as usize, "range", 1, "1-3,1-4294967295,2-4294967295", Some(5)).unwrap();
    assert_eq!(idx, vec![0, 1, 2, 3, 4]);
    // A later token that is out of range is still an error.
    assert!(parse::select_records(10, "range", 1, "1-10,5-11", Some(2)).is_err());
    assert!(parse::select_records(10, "range", 1, "1-2,0", Some(2)).is_err());
}
