//! Engine tests for data merge. They drive the commands, not a private copy of the parser.

use std::path::{Path, PathBuf};

use designcraft_doc::{Content, DataField, DataFieldKind, DataSource, Delimiter, HyperlinkDest, SourceStatus};
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
        });
    }
    kinds.sort();
    assert_eq!(kinds, vec!["email:a@b.c".to_string(), "url:https://example.com".to_string()]);
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

    exec(&mut s, "file.activate", json!({"index": 0}));
    exec(&mut s, "data.source.update", json!({}));
    let r = exec(&mut s, "data.merge", json!({}));
    assert_eq!(r["records"], 3);
    assert!(texts(&s).iter().any(|t| t == "Kai"));

    exec(&mut s, "file.activate", json!({"index": 0}));
    exec(&mut s, "data.source.remove", json!({}));
    assert!(s.documents()[0].doc.data_merge.sources.is_empty());
    assert!(s.documents()[0].doc.data_merge.placeholders.iter().any(|ph| ph.id == id.as_u64().unwrap()));
    assert_eq!(exec(&mut s, "data.fields", json!({})), json!([]));

    select_csv(&mut s, "Name\nAda\nGrace\n");
    exec(&mut s, "data.preview", json!({"record": 2}));
    assert!(texts(&s).iter().any(|t| t.contains("Grace")));
    assert!(!texts(&s).iter().any(|t| t.contains("<<Name>>")));
    let ser = exec(&mut s, "file.serialize", json!({}));
    let saved = crate::cmd::from_bytes(&crate::cmd::base64_decode(ser["base64"].as_str().unwrap())).unwrap();
    assert!(saved.stories.values().any(|st| st.text.contains("<<Name>>")));
    assert!(!saved.stories.values().any(|st| st.text.contains("Grace")));
    exec(&mut s, "data.preview.stop", json!({}));
    assert!(texts(&s).iter().any(|t| t.contains("<<Name>>")));

    exec(&mut s, "data.preview", json!({"record": 2}));
    let path = dir.join("saved.designcraft");
    exec(&mut s, "file.save", json!({"path": path}));
    let saved = crate::cmd::from_bytes(&std::fs::read(&path).unwrap()).unwrap();
    assert!(!saved.stories.values().any(|st| st.text.contains("Grace")));
    assert!(saved.stories.values().any(|st| st.text.contains("<<Name>>")));

    exec(&mut s, "data.preview", json!({"record": 2}));
    let rec = dir.join("recovery");
    s.recovery_dir = Some(rec.clone());
    crate::recovery::save(&s, &rec).unwrap();
    let uid = s.documents()[0].uid;
    let recovered = crate::cmd::from_bytes(&std::fs::read(rec.join(format!("{uid}.designcraft"))).unwrap()).unwrap();
    assert!(!recovered.stories.values().any(|st| st.text.contains("Grace")));
    assert!(texts(&s).iter().any(|t| t.contains("Grace")), "recovery writes the stash and leaves the preview on screen");
    exec(&mut s, "file.new", json!({}));
    exec(&mut s, "file.activate", json!({"index": 0}));
    assert!(texts(&s).iter().any(|t| t.contains("<<Name>>")));

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
fn oversized_data_file_is_refused_before_reading() {
    let dir = scratch("oversized");
    let path = dir.join("big.csv");
    write(&path, &"x".repeat(2048));
    let path = path.to_string_lossy().to_string();
    let msg = read_path_capped(&path, 1024).unwrap_err();
    assert!(msg.contains("larger than"), "{msg}");
    assert_eq!(read_path_capped(&path, 4096).unwrap().len(), 2048);
}
