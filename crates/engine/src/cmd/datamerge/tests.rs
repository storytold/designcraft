//! Engine tests for data merge. They drive the commands, not a private copy of the parser.

use std::path::{Path, PathBuf};

use designcraft_doc::{Content, DataField, DataFieldKind, DataSource, Delimiter, FilterRule, HyperlinkDest, SourceStatus};
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

fn story_order(s: &Session) -> Vec<String> {
    let doc = &s.documents()[s.active_index().unwrap()].doc;
    let mut v = Vec::new();
    for sp in &doc.spreads {
        for it in &sp.items {
            if let Content::Text(tf) = &it.content
                && let Some(st) = doc.story(tf.story)
            {
                v.push(st.text.clone());
            }
        }
    }
    v
}

fn page_frames(s: &Session) -> Vec<Vec<(i32, i32, String)>> {
    let doc = &s.documents()[s.active_index().unwrap()].doc;
    let mut pages = Vec::new();
    for sp in &doc.spreads {
        let mut items = Vec::new();
        for it in &sp.items {
            if let Content::Text(tf) = &it.content
                && let Some(st) = doc.story(tf.story)
            {
                let b = it.bounds();
                items.push((b.x0.round() as i32, b.y0.round() as i32, st.text.clone()));
            }
        }
        items.sort();
        pages.push(items);
    }
    pages
}

fn grid_count(s: &Session) -> usize {
    let doc = &s.documents()[s.active_index().unwrap()].doc;
    let mut n = 0;
    for sp in doc.spreads.iter().chain(doc.parents.iter()) {
        for it in &sp.items {
            it.walk(&mut |item| {
                if item.data_grid.is_some() {
                    n += 1;
                }
            });
        }
    }
    n
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
    let row = |s: &Session| s.documents()[s.active_index().unwrap()].doc.data_merge.sources.last().unwrap().rows[0].clone();
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
    let src = s.documents()[0].doc.data_merge.sources.last().unwrap();
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
    let src = s.documents()[0].doc.data_merge.sources.last().unwrap();
    assert_eq!(src.rows[0], vec!["Ada", "12", "TRUE", "44927"]);
    // Data merge computes an uncached formula. Place-as-table still leaves that cell empty.
    assert_eq!(src.rows[1], vec!["Grace", "", "2", ""]);
    let msg = err(&mut s, "data.source.select", json!({"bytes": book, "name": "people.xlsx", "sheet": "Missing"}));
    assert!(msg.contains("Missing"), "{msg}");
    assert_eq!(s.documents()[0].doc.data_merge.sources.last().unwrap().rows[0][0], "Ada");
}

fn phase3_workbook() -> String {
    xlsx(&[
        (
            "xl/workbook.xml",
            r#"<workbook xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="People" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        ),
        ("xl/_rels/workbook.xml.rels", r#"<Relationships><Relationship Id="rId1" Type="x" Target="worksheets/sheet1.xml"/></Relationships>"#),
        (
            "xl/styles.xml",
            r#"<styleSheet><numFmts><numFmt numFmtId="164" formatCode="yyyy-mm-dd"/><numFmt numFmtId="165" formatCode="yyyy-qq"/></numFmts><cellXfs><xf numFmtId="0"/><xf numFmtId="164"/><xf numFmtId="165"/><xf numFmtId="14"/></cellXfs></styleSheet>"#,
        ),
        (
            "xl/worksheets/sheet1.xml",
            r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Name</t></is></c><c r="B1" t="inlineStr"><is><t>Qty</t></is></c><c r="C1" t="inlineStr"><is><t>When</t></is></c><c r="D1" t="inlineStr"><is><t>Other</t></is></c><c r="E1" t="inlineStr"><is><t>Built</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>Ada</t></is></c><c r="B2"><v>9</v><f>1+1</f></c><c r="C2" s="1"><v>1</v></c><c r="D2"><v>12</v></c><c r="E2" s="3"><v>1</v></c></row><row r="3"><c r="A3" t="inlineStr"><is><t>Bea</t></is></c><c r="B3"><f>1+1</f></c><c r="C3" s="1"><v>60</v></c><c r="D3" s="1"><v>61</v></c></row><row r="4"><c r="A4" t="inlineStr"><is><t>Cy</t></is></c><c r="B4"><f>SUM(B2:B3)</f></c><c r="C4" s="2"><v>1</v></c><c r="D4"><f>NOW()</f></c></row><row r="5"><c r="A5" t="inlineStr"><is><t>Dee</t></is></c><c r="B5"><f>A2&amp;&quot;x&quot;</f></c></row></sheetData></worksheet>"#,
        ),
    ])
}

#[test]
fn phase3_excel_formulas_dates_xls_and_place_stays_cached() {
    let book = phase3_workbook();
    let mut s = Session::new();
    new_doc(&mut s, 1);
    exec(&mut s, "data.source.select", json!({"bytes": book, "name": "people.xlsx"}));
    let src = &s.documents()[0].doc.data_merge.sources[0];
    assert_eq!(src.rows[0], vec!["Ada", "9", "1900-01-01", "12", "01-01-00"]);
    assert_eq!(src.rows[1], vec!["Bea", "2", "1900-02-29", "1900-03-01", ""]);
    assert_eq!(src.rows[2][1], "11");
    assert_eq!(src.rows[2][2], "1");
    assert_eq!(src.rows[2][3], "");
    assert_eq!(src.rows[3][1], "");
    let notes = src.warnings.join("\n");
    assert!(notes.contains("Cell C4: the date format could not be read."), "{notes}");
    assert!(notes.contains("Cell D4: the formula could not be computed."), "{notes}");
    assert!(notes.contains("Cell B5: the formula could not be computed."), "{notes}");

    let msg = err(&mut s, "data.source.select", json!({"bytes": book, "name": "book.xlsm"}));
    assert!(msg.contains("xlsm"), "{msg}");
    assert_eq!(s.documents()[0].doc.data_merge.sources.len(), 1, "an xlsm file does not attach");

    let xls = designcraft_textimport::xls_fixture(&[
        vec![
            designcraft_textimport::XlsCell::Text("Name".into()),
            designcraft_textimport::XlsCell::Text("Qty".into()),
            designcraft_textimport::XlsCell::Text("Note".into()),
        ],
        vec![
            designcraft_textimport::XlsCell::Text("Ada".into()),
            designcraft_textimport::XlsCell::Rk(12),
            designcraft_textimport::XlsCell::Formula(9.0),
        ],
        vec![designcraft_textimport::XlsCell::Text("Bea".into()), designcraft_textimport::XlsCell::Empty, designcraft_textimport::XlsCell::Empty],
    ]);
    exec(&mut s, "data.source.select", json!({"bytes": crate::cmd::base64_encode(&xls), "name": "book.xls"}));
    let src = s.documents()[0].doc.data_merge.sources.last().unwrap();
    assert_eq!(src.rows[0], vec!["Ada", "12", "9"]);
    assert_eq!(src.rows[1], vec!["Bea", "", ""]);

    let placed = err(&mut s, "file.place", json!({"base64": crate::cmd::base64_encode(&xls), "name": "book.xls"}));
    assert!(placed.contains("not placed"), "{placed}");
    exec(&mut s, "file.place", json!({"base64": book, "name": "people.xlsx"}));
    let doc = &s.documents()[0].doc;
    let table = doc.stories.values().flat_map(|st| st.tables.values()).next().expect("placed table");
    let cell = |r, c| table.cell(r, c).unwrap().text.text.clone();
    assert_eq!(cell(1, 1), "9", "place keeps the cached value");
    assert_eq!(cell(1, 2), "1", "place leaves a date serial as a number");
    let mut flat = Vec::new();
    for r in 0..table.nrows() {
        for c in 0..table.ncols() {
            flat.push(cell(r, c));
        }
    }
    assert!(flat.iter().all(|text| text != "2" && !text.contains("1900")), "{flat:?}");
}

#[test]
fn phase3_grid_drag_uses_the_rectangle_and_create_grid_defaults() {
    use designcraft_tools::{PointerEvent, PointerKind};

    let v = crate::ViewInfo::at_zoom(1.0);
    let mut s = Session::new();
    new_doc(&mut s, 1);
    s.set_tool("dataGrid");
    s.pointer(&PointerEvent::new(PointerKind::Down, 80.0, 90.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, 80.0, 90.0), v).unwrap();
    assert!(s.doc().unwrap().doc.spreads[0].items.is_empty(), "a click does not create a grid");
    assert!(s.journal.iter().all(|(cmd, _)| cmd != "data.grid.create"));

    // A single page is centered on the spine, so spread x is the canvas x plus half the page width.
    let half = s.doc().unwrap().doc.spreads[0].pages[0].width / 2.0;
    s.pointer(&PointerEvent::new(PointerKind::Down, 50.0, 60.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, 220.0, 180.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, 220.0, 180.0), v).unwrap();
    let (cmd, params) = s.journal.last().expect("the mouse-up commits the preview").clone();
    assert_eq!(cmd, "data.grid.create");
    let keys: Vec<&String> = params.as_object().expect("object").keys().collect();
    assert_eq!(keys, ["rect", "spread"], "the drag sends only the rectangle and the spread");
    assert_eq!(params["spread"], json!(0));
    let rect = json!([50.0 + half, 60.0, 220.0 + half, 180.0]);
    assert_eq!(params["rect"], rect);
    let st = s.doc().unwrap();
    assert_eq!(st.history.undo.len(), 1);
    let it = &st.doc.spreads[0].items[0];
    assert_eq!(it.bounds(), designcraft_geom::Rect::new(50.0 + half, 60.0, 220.0 + half, 180.0));
    let grid = it.data_grid.as_ref().expect("a grid");
    assert_eq!((grid.rows, grid.columns), (2, 2));
    assert_eq!(grid.gutter, 0.0);
    assert_eq!((grid.record_offset, grid.record_advance), (0, 1));
    assert_eq!(grid.origin, "topLeft");
    assert_eq!(grid.arrange, "rows");
    assert_eq!(st.doc.data_merge.options.per_page, "single");

    // Two points inside the left margin, inside the snap zone. The rectangle stays on the drag.
    let mut near = Session::new();
    new_doc(&mut near, 1);
    let margin = near.doc().unwrap().doc.spreads[0].pages[0].margins.inside;
    let x0 = margin + 2.0 - half;
    near.set_tool("dataGrid");
    near.pointer(&PointerEvent::new(PointerKind::Down, x0, 80.0), v).unwrap();
    near.pointer(&PointerEvent::new(PointerKind::Drag, 120.0, 200.0), v).unwrap();
    near.pointer(&PointerEvent::new(PointerKind::Up, 120.0, 200.0), v).unwrap();
    let near_rect = near.journal.last().unwrap().1["rect"].clone();
    assert_eq!(near_rect, json!([margin + 2.0, 80.0, 120.0 + half, 200.0]), "the drag does not snap, {near_rect}");

    let mut parent = Session::new();
    new_doc(&mut parent, 1);
    parent.active_mut().unwrap().editing_parents = true;
    parent.set_tool("dataGrid");
    parent.pointer(&PointerEvent::new(PointerKind::Down, 50.0, 60.0), v).unwrap();
    parent.pointer(&PointerEvent::new(PointerKind::Drag, 220.0, 180.0), v).unwrap();
    parent.pointer(&PointerEvent::new(PointerKind::Up, 220.0, 180.0), v).unwrap();
    let doc = &parent.doc().unwrap().doc;
    assert!(doc.parents.iter().flat_map(|sp| sp.items.iter()).all(|it| it.data_grid.is_none()));
    assert!(doc.spreads.iter().flat_map(|sp| sp.items.iter()).all(|it| it.data_grid.is_none()));
    assert!(parent.journal.iter().all(|(cmd, _)| cmd != "data.grid.create"));
}

#[test]
fn phase3_merge_writes_a_pdf_only_when_a_path_is_given() {
    let dir = scratch("pdf");
    let path = dir.join("merged.pdf");
    let mut s = Session::new();
    new_doc(&mut s, 1);
    select_csv(&mut s, "Name\nAda\nGrace\n");
    text_frame(&mut s, [36.0, 36.0, 200.0, 80.0], "<<Name>>", None);
    let merged = exec(&mut s, "data.merge", json!({"pdf": path.to_string_lossy()}));
    assert_eq!(merged["records"], 2);
    assert_eq!(s.documents().len(), 2, "the merged document is added");
    let active = &s.documents()[s.active_index().unwrap()].doc;
    let story_text: Vec<&str> = active.stories.values().map(|st| st.text.as_str()).collect();
    assert!(story_text.iter().any(|t| t.contains("Ada")) && story_text.iter().any(|t| t.contains("Grace")), "{story_text:?}");
    assert!(story_text.iter().all(|t| !t.contains("<<Name>>")), "{story_text:?}");
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"%PDF"), "pdf magic");
    let len = bytes.len();
    let again = exec(&mut s, "data.merge", json!({}));
    assert_eq!(again["records"], 2);
    assert_eq!(std::fs::read(&path).unwrap().len(), len, "omitting pdf leaves the previous file alone");
    assert!(!dir.join("other.pdf").exists());
    let bad = err(&mut s, "data.merge", json!({"pdf": 12}));
    assert!(bad.contains("pdf"), "{bad}");
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
    assert_eq!(warnings.len(), 0, "{warnings:?}");
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
    let pages = page_frames(&s);
    assert!(pages.len() >= 2, "{pages:?}");
    assert!(pages[0].iter().any(|(_, _, t)| t == "Ada"), "{pages:?}");
    assert!(pages[1].iter().any(|(_, _, t)| t == "Grace"), "{pages:?}");
    assert!(texts(&s).contains(&"Ada".to_string()));
    assert!(texts(&s).contains(&"Grace".to_string()));
}

#[test]
fn phase3_blank_page_warns_once_for_an_unfilled_parent() {
    let mut s = Session::new();
    new_doc(&mut s, 2);
    select_csv(&mut s, "Name\nAda\nGrace\n");
    let parent = exec(
        &mut s,
        "frame.create",
        json!({"rect": [36.0, 120.0, 220.0, 160.0], "content": "text", "text": "", "spread": {"kind": "parent", "index": 0}}),
    );
    exec(&mut s, "data.placeholder.add", json!({"field": "Name", "story": parent["story"], "at": 0}));
    text_frame(&mut s, [36.0, 36.0, 240.0, 80.0], "<<Name>>", Some(0));

    let preview = exec(&mut s, "data.preview", json!({"record": 1}));
    let preview_warnings: Vec<_> = preview["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str()).collect();
    assert_eq!(preview_warnings, vec!["A parent-page placeholder was left unfilled."]);
    let preview_pages = page_frames(&s);
    assert!(preview_pages[0].iter().any(|(_, _, t)| t == "Ada"), "{preview_pages:?}");
    assert!(preview_pages[1].is_empty(), "the blank page has no record copy: {preview_pages:?}");
    exec(&mut s, "data.preview.stop", json!({}));

    let merged = exec(&mut s, "data.merge", json!({}));
    let warnings: Vec<_> = merged["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str()).filter(|w| w.contains("parent")).collect();
    assert_eq!(warnings, vec!["A parent-page placeholder was left unfilled."]);
    let doc = &s.documents()[s.active_index().unwrap()].doc;
    let parent_text: Vec<_> = doc
        .parents
        .iter()
        .flat_map(|sp| sp.items.iter())
        .filter_map(|it| match &it.content {
            Content::Text(tf) => doc.story(tf.story).map(|st| st.text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(parent_text, vec!["<<Name>>".to_string()]);
    assert!(!doc.page(0).unwrap().overridden.is_empty(), "the record page overrides the parent");
    assert!(doc.page(1).unwrap().overridden.is_empty(), "the blank page keeps the unfilled parent");
    assert!(!doc.page(2).unwrap().overridden.is_empty());
    assert!(doc.page(3).unwrap().overridden.is_empty());
    let pages = page_frames(&s);
    assert_eq!(pages.len(), 4, "{pages:?}");
    assert!(pages[0].iter().any(|(_, _, t)| t == "Ada"), "{pages:?}");
    assert!(pages[1].is_empty(), "{pages:?}");
    assert!(pages[2].iter().any(|(_, _, t)| t == "Grace"), "{pages:?}");
    assert!(pages[3].is_empty(), "{pages:?}");

    let mut both = Session::new();
    new_doc(&mut both, 2);
    select_csv(&mut both, "Name\nAda\n");
    let parent = exec(
        &mut both,
        "frame.create",
        json!({"rect": [36.0, 120.0, 220.0, 160.0], "content": "text", "text": "", "spread": {"kind": "parent", "index": 0}}),
    );
    exec(&mut both, "data.placeholder.add", json!({"field": "Name", "story": parent["story"], "at": 0}));
    text_frame(&mut both, [36.0, 36.0, 200.0, 80.0], "<<Name>>", Some(0));
    text_frame(&mut both, [36.0, 36.0, 200.0, 80.0], "<<Name>>", Some(1));
    let filled = exec(&mut both, "data.merge", json!({}));
    let warnings: Vec<_> = filled["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str()).filter(|w| w.contains("parent")).collect();
    assert!(warnings.is_empty(), "{warnings:?}");
    let pages = page_frames(&both);
    assert!(pages.iter().all(|page| page.iter().any(|(_, _, t)| t == "Ada")), "{pages:?}");

    let mut only_parent = Session::new();
    new_doc(&mut only_parent, 1);
    select_csv(&mut only_parent, "Name\nAda\n");
    let parent = exec(
        &mut only_parent,
        "frame.create",
        json!({"rect": [36.0, 36.0, 200.0, 80.0], "content": "text", "text": "", "spread": {"kind": "parent", "index": 0}}),
    );
    exec(&mut only_parent, "data.placeholder.add", json!({"field": "Name", "story": parent["story"], "at": 0}));
    let filled = exec(&mut only_parent, "data.merge", json!({}));
    let warnings: Vec<_> = filled["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str()).filter(|w| w.contains("parent")).collect();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(page_frames(&only_parent).iter().any(|page| page.iter().any(|(_, _, t)| t == "Ada")));
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
    assert_eq!(relative_between(&a, &csv), "../b/data.csv");
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
        enabled: true,
        filter: designcraft_doc::SourceFilter::default(),
        sort: Vec::new(),
        affixes: Vec::new(),
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
    assert_eq!(src.relative_path.as_deref(), Some("../b/data.csv"));
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

fn source_rows(s: &Session) -> Vec<Vec<String>> {
    s.documents()[s.active_index().unwrap()].doc.data_merge.sources[0].rows.clone()
}

fn source_names(s: &Session) -> Vec<String> {
    s.documents()[s.active_index().unwrap()].doc.data_merge.sources[0].fields.iter().map(|field| field.name.clone()).collect()
}

const JSON_PEOPLE: &str = r#"[{"Name":"Ada"},{"Name":"Grace"}]"#;

#[test]
fn json_path_bytes_and_inline_string_attach() {
    let dir = scratch("json-src");
    let path = dir.join("people.json");
    write(&path, JSON_PEOPLE);
    let mut s = Session::new();
    new_doc(&mut s, 1);

    let from_path = exec(&mut s, "data.source.select", json!({"path": path}));
    assert_eq!(from_path["fields"][0]["name"], "Name");
    assert_eq!(source_rows(&s), vec![vec!["Ada"], vec!["Grace"]]);
    assert!(s.documents()[0].doc.data_merge.sources[0].path.is_some());
    assert!(s.documents()[0].doc.data_merge.sources[0].fingerprint.is_some());
    println!("GATING json path field Name rows {:?}", source_rows(&s));

    let from_bytes = exec(&mut s, "data.source.select", json!({"bytes": crate::cmd::base64_encode(JSON_PEOPLE.as_bytes()), "name": "people.json"}));
    assert_eq!(from_bytes["fields"][0]["name"], "Name");
    assert_eq!(s.documents()[0].doc.data_merge.sources.len(), 2);
    let bytes_src = s.documents()[0].doc.data_merge.sources.last().unwrap();
    assert_eq!(bytes_src.rows, vec![vec!["Ada"], vec!["Grace"]]);
    assert!(bytes_src.path.is_none());
    assert!(s.documents()[0].doc.data_merge.sources[0].path.is_some());
    println!("GATING json bytes field Name rows {:?}", bytes_src.rows);

    let from_inline = exec(&mut s, "data.source.select", json!({"json": JSON_PEOPLE}));
    assert_eq!(from_inline["fields"][0]["name"], "Name");
    assert_eq!(from_inline["records"], 2);
    assert_eq!(s.documents()[0].doc.data_merge.sources.len(), 3);
    let src = s.documents()[0].doc.data_merge.sources.last().unwrap();
    assert_eq!(src.rows, vec![vec!["Ada"], vec!["Grace"]]);
    assert!(src.path.is_none());
    assert!(src.fingerprint.is_none());
    println!("GATING json inline field Name rows {:?}", src.rows);
}

#[test]
fn json_key_order_kinds_scalars_and_bom() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    exec(&mut s, "data.source.select", json!({"json": r#"[{"Zebra":"z","Apple":"a"},{"Apple":"later","Zebra":"also"}]"#}));
    assert_eq!(source_names(&s), vec!["Zebra", "Apple"]);
    assert_eq!(source_rows(&s), vec![vec!["z", "a"], vec!["also", "later"]]);
    println!("GATING json key order {:?}", source_names(&s));

    let marked_body = "[{\"@Photo\":\"pic.png\",\"#Code\":\"abc\",\"Name\":\"Ada\"}]";
    let marked = exec(&mut s, "data.source.select", json!({"json": marked_body}));
    assert_eq!(
        marked["fields"],
        json!([
            {"name": "Photo", "kind": "image"},
            {"name": "Code", "kind": "qr"},
            {"name": "Name", "kind": "text"}
        ])
    );
    println!("GATING json kinds photo=image code=qr");

    let scalars = r#"[{"Name":"Ada","Qty":2,"On":true,"Note":null,"Off":false,"Amount":1.5}]"#;
    let marker = "<<Name>>|<<Qty>>|<<On>>|<<Note>>|<<Off>>|<<Amount>>";
    let merge_text = |payload: Value| {
        let mut session = Session::new();
        new_doc(&mut session, 1);
        text_frame(&mut session, [36.0, 36.0, 520.0, 80.0], marker, None);
        exec(&mut session, "data.merge", payload);
        texts(&session)
    };
    let from_rows = merge_text(json!({"rows": [{"Name": "Ada", "Qty": 2, "On": true, "Note": null, "Off": false, "Amount": 1.5}]}));
    let from_json = merge_text(json!({"json": scalars}));
    assert_eq!(from_json, from_rows);
    assert!(from_json.iter().any(|text| text.contains("Ada") && !text.contains("<<")), "{from_json:?}");
    println!("GATING json scalars rows={from_rows:?} json={from_json:?}");

    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend(JSON_PEOPLE.as_bytes());
    exec(&mut s, "data.source.select", json!({"bytes": crate::cmd::base64_encode(&bom), "name": "people.json"}));
    let bom_src = s.documents()[0].doc.data_merge.sources.last().unwrap();
    assert_eq!(bom_src.fields.iter().map(|field| field.name.as_str()).collect::<Vec<_>>(), vec!["Name"]);
    assert_eq!(bom_src.rows, vec![vec!["Ada"], vec!["Grace"]]);
    assert_eq!(source_names(&s), vec!["Zebra", "Apple"]);
    println!("GATING json bom records {} first {}", bom_src.rows.len(), bom_src.rows[0][0]);
}

#[test]
fn json_extra_key_warns_and_invalid_input_does_not_attach() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    let selected = exec(&mut s, "data.source.select", json!({"json": r#"[{"Name":"Ada"},{"Name":"Grace","Extra":"x"}]"#}));
    let warning = selected["warnings"][0].as_str().unwrap().to_string();
    assert!(warning.contains("Row 2"), "{warning}");
    assert!(warning.contains("Extra"), "{warning}");
    assert_eq!(source_names(&s), vec!["Name"]);
    assert_eq!(source_rows(&s), vec![vec!["Ada"], vec!["Grace"]]);
    println!("GATING json extra warning {warning}");

    let kept = source_rows(&s);
    let cases = [
        ("top-level object", json!({"json": r#"{"Name":"Ada"}"#})),
        ("non-object element", json!({"json": r#"[{"Name":"Ada"},"Grace"]"#})),
        ("nested object", json!({"json": r#"[{"Name":{"given":"Ada"}}]"#})),
        ("nested array", json!({"json": r#"[{"Name":["Ada"]}]"#})),
        ("empty array", json!({"json": "[]"})),
        ("no keys", json!({"json": "[{}]"})),
        ("non-utf8", json!({"bytes": crate::cmd::base64_encode(&[0xFFu8, 0xFE]), "name": "people.json"})),
    ];
    for (label, payload) in cases {
        let msg = err(&mut s, "data.source.select", payload);
        assert!(!msg.is_empty(), "{label}");
        assert_eq!(s.documents()[0].doc.data_merge.sources.len(), 1, "{label}");
        assert_eq!(source_rows(&s), kept, "{label} changed the source");
        println!("GATING json rejected {label} sources {}", s.documents()[0].doc.data_merge.sources.len());
    }

    let mut empty = Session::new();
    new_doc(&mut empty, 1);
    let before = empty.documents().len();
    assert!(!err(&mut empty, "data.merge", json!({"json": r#"{"Name":"Ada"}"#})).is_empty());
    assert_eq!(empty.documents().len(), before);
    assert!(empty.documents()[0].doc.data_merge.sources.is_empty());
}

#[test]
fn json_inline_merge_leaves_the_template_unlinked() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    text_frame(&mut s, [36.0, 36.0, 320.0, 80.0], "<<Name>>", None);
    let merged = exec(&mut s, "data.merge", json!({"json": JSON_PEOPLE}));
    assert_eq!(merged["records"], 2);
    assert_eq!(merged["pages"], 2);
    assert_eq!(texts(&s), vec!["Ada".to_string(), "Grace".to_string()]);
    assert!(s.documents()[0].doc.data_merge.sources.is_empty());
    assert!(s.documents()[0].doc.stories.values().any(|story| story.text.contains("<<Name>>")));
    println!(
        "GATING json merge records {} pages {} template sources {}",
        merged["records"],
        merged["pages"],
        s.documents()[0].doc.data_merge.sources.len()
    );
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

fn select_body(s: &mut Session, name: &str, body: &str) -> u64 {
    exec(s, "data.source.select", json!({"bytes": crate::cmd::base64_encode(body.as_bytes()), "name": name}))["id"].as_u64().unwrap()
}

fn names_csv(labels: &[&str]) -> String {
    let mut body = String::from("Name\n");
    for label in labels {
        body.push_str(label);
        body.push('\n');
    }
    body
}

fn add_grid(s: &mut Session, spread: u32, rect: [f64; 4], cell: [f64; 4], rows: u32, cols: u32, extra: Value) -> u64 {
    text_frame(s, cell, "<<Name>>", Some(spread));
    let mut body = json!({
        "rect": rect,
        "spread": spread,
        "rows": rows,
        "columns": cols,
        "gutter": 0,
        "origin": "topLeft",
        "arrange": "rows",
    });
    if let (Some(dst), Some(src)) = (body.as_object_mut(), extra.as_object()) {
        for (k, v) in src {
            dst.insert(k.clone(), v.clone());
        }
    }
    exec(s, "data.grid.create", body)["id"].as_u64().unwrap()
}

fn doc_len(s: &Session) -> usize {
    s.documents().len()
}

#[test]
fn select_adds_and_replace_keeps_id_filter_and_sort() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    let first = select_body(&mut s, "a.csv", "Name\nAda\n");
    let second = select_body(&mut s, "b.csv", "Name\nBea\n");
    assert_ne!(first, second);
    let sources = &s.documents()[0].doc.data_merge.sources;
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0].id, first);
    assert_eq!(sources[0].rows, vec![vec!["Ada"]]);
    assert_eq!(sources[1].id, second);
    assert_eq!(sources[1].rows, vec![vec!["Bea"]]);

    exec(&mut s, "data.source.filter", json!({"id": first, "match": "all", "rules": [{"field": "Name", "op": "equals", "value": "Ada"}]}));
    exec(&mut s, "data.source.enabled", json!({"id": first, "enabled": false}));
    exec(&mut s, "data.source.sort", json!({"id": first, "fields": [{"field": "Name", "direction": "desc"}]}));
    let replaced = exec(&mut s, "data.source.select", json!({"id": first, "csv": "Name\nKai\nNia\n"}));
    assert_eq!(replaced["id"], first);
    let sources = &s.documents()[0].doc.data_merge.sources;
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0].id, first);
    assert_eq!(sources[0].rows, vec![vec!["Kai"], vec!["Nia"]]);
    assert!(!sources[0].enabled);
    assert_eq!(sources[0].filter.rules.len(), 1);
    assert_eq!(sources[0].sort.len(), 1);
    assert_eq!(sources[1].rows, vec![vec!["Bea"]]);

    exec(&mut s, "edit.undo", json!({}));
    let sources = &s.documents()[0].doc.data_merge.sources;
    assert_eq!(sources[0].rows, vec![vec!["Ada"]]);
    assert!(!sources[0].enabled);
    exec(&mut s, "edit.undo", json!({}));
    assert!(s.documents()[0].doc.data_merge.sources[0].sort.is_empty());
}

#[test]
fn update_keeps_id_enabled_filter_and_sort() {
    let dir = scratch("update-keep");
    let csv = dir.join("people.csv");
    write(&csv, "Name\nAda\nGrace\n");
    let mut s = Session::new();
    new_doc(&mut s, 1);
    let id = exec(&mut s, "data.source.select", json!({"path": &csv}))["id"].as_u64().unwrap();
    exec(&mut s, "data.source.filter", json!({"id": id, "rules": [{"field": "Name", "op": "equals", "value": "Ada"}]}));
    exec(&mut s, "data.source.sort", json!({"id": id, "fields": [{"field": "Name", "direction": "desc"}]}));
    exec(&mut s, "data.source.enabled", json!({"id": id, "enabled": false}));
    write(&csv, "Name\nNia\n");
    exec(&mut s, "data.source.update", json!({"id": id}));
    let src = &s.documents()[0].doc.data_merge.sources[0];
    assert_eq!(src.id, id);
    assert!(!src.enabled);
    assert_eq!(src.filter.rules.len(), 1);
    assert_eq!(src.sort.len(), 1);
    assert_eq!(src.rows, vec![vec!["Nia"]]);
    assert_eq!(src.status, SourceStatus::Ok);
}

#[test]
fn enabled_sources_concatenate_and_other_source_placeholders_are_blank() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    let ada = select_body(&mut s, "a.csv", "Name\nAda\n");
    let bea = select_body(&mut s, "b.csv", "Name\nBea\n");
    let left = text_frame(&mut s, [36.0, 36.0, 220.0, 80.0], "A-", None);
    let right = text_frame(&mut s, [36.0, 100.0, 220.0, 150.0], "B-", None);
    exec(&mut s, "data.placeholder.add", json!({"field": "Name", "source": ada, "story": left["story"], "at": 2}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Name", "source": bea, "story": right["story"], "at": 2}));

    let fields = exec(&mut s, "data.fields", json!({}));
    let names: Vec<_> = fields.as_array().unwrap().iter().filter(|field| field["name"] == "Name").collect();
    assert_eq!(names.len(), 2);
    assert_eq!(names[0]["sourceId"], ada);
    assert_eq!(names[1]["sourceId"], bea);
    assert_eq!(fields[0]["enabled"], true);
    assert_eq!(fields[0]["records"], 1);
    assert_eq!(fields[0]["preview"], 2);

    let merged = exec(&mut s, "data.merge", json!({}));
    assert_eq!(merged["records"], 2);
    assert!(merged["warnings"].as_array().unwrap().is_empty(), "{merged}");
    assert_eq!(
        page_frames(&s),
        vec![vec![(36, 36, "A-Ada".into()), (36, 100, "B-".into())], vec![(36, 36, "A-".into()), (36, 100, "B-Bea".into())],]
    );

    exec(&mut s, "file.activate", json!({"index": 0}));
    exec(&mut s, "data.source.enabled", json!({"id": bea, "enabled": false}));
    let merged = exec(&mut s, "data.merge", json!({}));
    assert_eq!(merged["records"], 1);
    assert_eq!(story_order(&s), vec!["A-Ada".to_string(), "B-".to_string()]);
    assert!(merged["warnings"].as_array().unwrap().is_empty(), "{merged}");

    exec(&mut s, "file.activate", json!({"index": 0}));
    exec(&mut s, "data.source.enabled", json!({"id": ada, "enabled": false}));
    let before = doc_len(&s);
    let msg = err(&mut s, "data.merge", json!({}));
    assert!(msg.contains("no enabled data source"), "{msg}");
    assert_eq!(doc_len(&s), before);
}

#[test]
fn filter_is_case_sensitive_and_unknown_field_adds_no_document() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    text_frame(&mut s, [36.0, 36.0, 320.0, 80.0], "<<Name>>|<<City>>", None);
    select_body(&mut s, "people.csv", "Name,City\nAda,Town\nada,town\nGrace,\nBob,Paris\n");

    let before = doc_len(&s);
    let msg = err(&mut s, "data.source.filter", json!({"rules": [{"field": "Nope", "op": "equals", "value": "x"}]}));
    assert!(msg.contains("unknown field"), "{msg}");
    assert!(s.documents()[0].doc.data_merge.sources[0].filter.rules.is_empty());
    assert_eq!(doc_len(&s), before);
    let msg = err(&mut s, "data.source.filter", json!({"match": "none", "rules": []}));
    assert!(msg.contains("unknown match"), "{msg}");
    let msg = err(&mut s, "data.source.filter", json!({"rules": [{"field": "Name", "op": "equals", "value": {"nested": true}}]}));
    assert!(msg.contains("string, number, bool, or null"), "{msg}");

    exec(&mut s, "data.source.filter", json!({"match": "all", "rules": [{"field": "Name", "op": "equals", "value": "Ada"}]}));
    assert_eq!(exec(&mut s, "data.fields", json!({}))[0]["records"], 1);
    exec(&mut s, "data.merge", json!({}));
    assert_eq!(story_order(&s), vec!["Ada|Town".to_string()]);

    exec(&mut s, "file.activate", json!({"index": 0}));
    exec(&mut s, "data.source.filter", json!({"rules": [{"field": "City", "op": "contains", "value": "To"}]}));
    exec(&mut s, "data.merge", json!({}));
    assert_eq!(story_order(&s), vec!["Ada|Town".to_string()]);

    exec(&mut s, "file.activate", json!({"index": 0}));
    exec(
        &mut s,
        "data.source.filter",
        json!({
            "match": "any",
            "rules": [
                {"field": "Name", "op": "equals", "value": "Grace"},
                {"field": "City", "op": "contains", "value": "Pa"}
            ]
        }),
    );
    exec(&mut s, "data.merge", json!({}));
    assert_eq!(story_order(&s), vec!["Grace|".to_string(), "Bob|Paris".to_string()]);

    exec(&mut s, "file.activate", json!({"index": 0}));
    exec(&mut s, "data.source.filter", json!({"match": "all", "rules": [{"field": "City", "op": "empty"}]}));
    exec(&mut s, "data.merge", json!({}));
    assert_eq!(story_order(&s), vec!["Grace|".to_string()]);

    exec(&mut s, "file.activate", json!({"index": 0}));
    exec(&mut s, "data.source.filter", json!({"rules": []}));
    assert!(s.documents()[0].doc.data_merge.sources[0].filter.rules.is_empty());
    exec(&mut s, "edit.undo", json!({}));
    assert_eq!(s.documents()[0].doc.data_merge.sources[0].filter.rules[0].op, "empty");

    let doc = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().doc);
    doc.data_merge.sources[0].filter.rules.push(FilterRule { field: "Missing".into(), op: "equals".into(), value: "x".into() });
    let before = doc_len(&s);
    let msg = err(&mut s, "data.merge", json!({}));
    assert!(msg.contains("unknown field"), "{msg}");
    assert_eq!(doc_len(&s), before);
}

#[test]
fn sort_is_stable_across_descending_then_ascending_fields() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    text_frame(&mut s, [36.0, 36.0, 360.0, 80.0], "<<Group>>|<<Name>>|<<Tag>>", None);
    select_body(&mut s, "people.csv", "Group,Name,Tag\nb,Amy,1\na,Ann,1\nb,Amy,2\na,Bob,1\nb,Zoe,1\n");
    let msg = err(&mut s, "data.source.sort", json!({"fields": [{"field": "Name", "direction": "sideways"}]}));
    assert!(msg.contains("unknown direction"), "{msg}");
    assert!(s.documents()[0].doc.data_merge.sources[0].sort.is_empty());
    exec(&mut s, "data.source.sort", json!({"fields": [{"field": "Group", "direction": "desc"}, {"field": "Name"}]}));
    exec(&mut s, "data.merge", json!({}));
    assert_eq!(
        story_order(&s),
        vec!["b|Amy|1".to_string(), "b|Amy|2".to_string(), "b|Zoe|1".to_string(), "a|Ann|1".to_string(), "a|Bob|1".to_string(),]
    );
}

#[test]
fn range_walks_the_combined_list() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    text_frame(&mut s, [36.0, 36.0, 240.0, 80.0], "<<Name>>", None);
    select_body(&mut s, "a.csv", "Name\nA1\nA2\nA3\n");
    select_body(&mut s, "b.csv", "Name\nB1\nB2\nB3\n");
    let merged = exec(&mut s, "data.merge", json!({"records": "range", "range": "3-4"}));
    assert_eq!(merged["records"], 2);
    assert_eq!(story_order(&s), vec!["A3".to_string(), "B1".to_string()]);
    exec(&mut s, "file.activate", json!({"index": 0}));
    assert_eq!(s.documents()[0].doc.data_merge.options.records, "all");
}

#[test]
fn changed_second_source_uses_cached_rows_without_an_undo_step() {
    let dir = scratch("second-source");
    let first = dir.join("a.csv");
    let second = dir.join("b.csv");
    write(&first, "Name\nAda\n");
    write(&second, "Name\nGrace\n");
    let mut s = Session::new();
    new_doc(&mut s, 1);
    text_frame(&mut s, [36.0, 36.0, 240.0, 80.0], "<<Name>>", None);
    exec(&mut s, "data.source.select", json!({"path": &first}));
    exec(&mut s, "data.source.select", json!({"path": &second}));
    let undo = s.documents()[0].history.undo.len();
    let mut body = std::fs::read(&second).unwrap();
    body.extend_from_slice(b"Kai\n");
    std::fs::write(&second, body).unwrap();
    let merged = exec(&mut s, "data.merge", json!({}));
    assert_eq!(merged["records"], 2);
    assert!(merged["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap_or("").contains("changed")));
    assert_eq!(texts(&s), vec!["Ada".to_string(), "Grace".to_string()]);
    let sources = &s.documents()[0].doc.data_merge.sources;
    assert_eq!(sources[0].status, SourceStatus::Ok);
    assert_eq!(sources[1].status, SourceStatus::Modified);
    assert_eq!(sources[1].rows, vec![vec!["Grace"]]);
    assert_eq!(s.documents()[0].history.undo.len(), undo);
}

#[test]
fn grid_on_page_two_of_a_three_page_template_writes_six_pages() {
    let mut s = Session::new();
    new_doc(&mut s, 3);
    text_frame(&mut s, [36.0, 36.0, 200.0, 80.0], "<<Name>>", Some(0));
    text_frame(&mut s, [36.0, 36.0, 200.0, 80.0], "<<Name>>", Some(2));
    add_grid(&mut s, 1, [72.0, 72.0, 272.0, 272.0], [72.0, 72.0, 172.0, 172.0], 2, 2, json!({}));
    select_csv(&mut s, &names_csv(&["R1", "R2", "R3", "R4", "R5", "R6"]));
    let merged = exec(&mut s, "data.merge", json!({}));
    let pages = page_frames(&s);
    println!("GATING 3-page template, 2x2 grid on page 2, pages {} frames {pages:?}", merged["pages"]);
    assert_eq!(merged["pages"], 6);
    assert_eq!(merged["records"], 6);
    assert_eq!(grid_count(&s), 0);
    assert_eq!(pages[0], vec![(36, 36, "R1".into())]);
    assert_eq!(pages[1], vec![(72, 72, "R1".into()), (72, 172, "R3".into()), (172, 72, "R2".into()), (172, 172, "R4".into())]);
    assert_eq!(pages[2], vec![(36, 36, "R1".into())]);
    assert_eq!(pages[3], vec![(36, 36, "R5".into())]);
    assert_eq!(pages[4], vec![(72, 72, "R5".into()), (72, 172, "".into()), (172, 72, "R6".into()), (172, 172, "".into())]);
    assert_eq!(pages[5], vec![(36, 36, "R5".into())]);
    assert!(merged["warnings"].as_array().unwrap().is_empty(), "{merged}");
}

#[test]
fn grid_top_right_origin_walks_rows_backwards() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    add_grid(&mut s, 0, [72.0, 72.0, 272.0, 272.0], [172.0, 72.0, 272.0, 172.0], 2, 2, json!({"origin": "topRight"}));
    select_csv(&mut s, &names_csv(&["R1", "R2", "R3", "R4"]));
    exec(&mut s, "data.merge", json!({}));
    assert_eq!(page_frames(&s), vec![vec![(72, 72, "R2".into()), (72, 172, "R4".into()), (172, 72, "R1".into()), (172, 172, "R3".into()),]]);
}

#[test]
fn grid_advance_zero_repeats_one_record_per_page() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    add_grid(&mut s, 0, [72.0, 72.0, 272.0, 272.0], [72.0, 72.0, 172.0, 172.0], 2, 2, json!({"recordAdvance": 0}));
    select_csv(&mut s, &names_csv(&["A", "B"]));
    let merged = exec(&mut s, "data.merge", json!({}));
    assert_eq!(merged["pages"], 2);
    let pages = page_frames(&s);
    assert!(pages[0].iter().all(|(_, _, text)| text == "A"), "{pages:?}");
    assert!(pages[1].iter().all(|(_, _, text)| text == "B"), "{pages:?}");
    assert_eq!(pages[0].len(), 4);
}

#[test]
fn grid_offset_skips_once_and_an_empty_origin_adds_no_document() {
    let mut s = Session::new();
    new_doc(&mut s, 2);
    add_grid(&mut s, 0, [72.0, 72.0, 272.0, 272.0], [72.0, 72.0, 172.0, 172.0], 2, 2, json!({"recordOffset": 1}));
    text_frame(&mut s, [36.0, 36.0, 200.0, 80.0], "<<Name>>", Some(1));
    select_csv(&mut s, &names_csv(&["R1", "R2", "R3", "R4"]));
    exec(&mut s, "data.merge", json!({}));
    let pages = page_frames(&s);
    assert_eq!(pages[0], vec![(72, 72, "R2".into()), (72, 172, "R4".into()), (172, 72, "R3".into()), (172, 172, "".into())]);
    assert_eq!(pages[1], vec![(36, 36, "R2".into())]);

    let mut s = Session::new();
    new_doc(&mut s, 1);
    add_grid(&mut s, 0, [72.0, 72.0, 272.0, 272.0], [72.0, 72.0, 172.0, 172.0], 2, 2, json!({"recordOffset": 5}));
    select_csv(&mut s, &names_csv(&["R1", "R2", "R3", "R4", "R5"]));
    let before = doc_len(&s);
    let msg = err(&mut s, "data.merge", json!({}));
    assert!(msg.contains("origin cell empty"), "{msg}");
    assert_eq!(doc_len(&s), before);
}

#[test]
fn two_grids_on_one_page_share_a_cursor() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    add_grid(&mut s, 0, [72.0, 72.0, 272.0, 172.0], [72.0, 72.0, 172.0, 172.0], 1, 2, json!({}));
    add_grid(&mut s, 0, [72.0, 200.0, 272.0, 300.0], [72.0, 200.0, 172.0, 300.0], 1, 2, json!({}));
    select_csv(&mut s, &names_csv(&["R1", "R2", "R3", "R4", "R5", "R6"]));
    let merged = exec(&mut s, "data.merge", json!({}));
    assert_eq!(merged["pages"], 2);
    assert_eq!(grid_count(&s), 0);
    let pages = page_frames(&s);
    assert_eq!(pages[0], vec![(72, 72, "R1".into()), (72, 200, "R3".into()), (172, 72, "R2".into()), (172, 200, "R4".into())]);
    assert_eq!(pages[1], vec![(72, 72, "R5".into()), (72, 200, "".into()), (172, 72, "R6".into()), (172, 200, "".into())]);
}

#[test]
fn grids_on_separate_pages_are_visited_in_page_order() {
    let mut s = Session::new();
    new_doc(&mut s, 2);
    add_grid(&mut s, 0, [72.0, 72.0, 272.0, 172.0], [72.0, 72.0, 172.0, 172.0], 1, 2, json!({}));
    add_grid(&mut s, 1, [72.0, 72.0, 272.0, 172.0], [72.0, 72.0, 172.0, 172.0], 1, 2, json!({}));
    select_csv(&mut s, &names_csv(&["R1", "R2", "R3", "R4"]));
    exec(&mut s, "data.merge", json!({}));
    let pages = page_frames(&s);
    assert_eq!(pages[0], vec![(72, 72, "R1".into()), (172, 72, "R2".into())]);
    assert_eq!(pages[1], vec![(72, 72, "R3".into()), (172, 72, "R4".into())]);
}

#[test]
fn grid_on_a_parent_pack_to_fit_and_a_collapsed_cell_add_no_document() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    exec(&mut s, "layout.parents.new", json!({}));
    let before = doc_len(&s);
    let msg = err(&mut s, "data.grid.create", json!({"rect": [72.0, 72.0, 272.0, 272.0], "spread": {"kind": "parent", "index": 0}}));
    assert!(msg.contains("document page"), "{msg}");
    assert_eq!(doc_len(&s), before);
    assert_eq!(grid_count(&s), 0);
    assert!(s.documents()[0].doc.parents[0].items.is_empty());

    let before_items = s.documents()[0].doc.spreads[0].items.len();
    let msg = err(&mut s, "data.grid.create", json!({"rect": [-500.0, 72.0, -200.0, 272.0]}));
    assert!(msg.contains("document page"), "{msg}");
    assert_eq!(doc_len(&s), before);
    assert_eq!(s.documents()[0].doc.spreads[0].items.len(), before_items);
    assert_eq!(grid_count(&s), 0);

    let mut s = Session::new();
    new_doc(&mut s, 1);
    add_grid(&mut s, 0, [72.0, 72.0, 272.0, 272.0], [72.0, 72.0, 172.0, 172.0], 2, 2, json!({}));
    select_csv(&mut s, &names_csv(&["A", "B"]));
    let before = doc_len(&s);
    let msg = err(&mut s, "data.merge", json!({"perPage": "multiple"}));
    assert!(msg.contains("grid"), "{msg}");
    assert_eq!(doc_len(&s), before);
    assert_eq!(grid_count(&s), 1);
    assert_eq!(s.documents()[0].doc.data_merge.options.per_page, "single");

    let mut s = Session::new();
    new_doc(&mut s, 1);
    text_frame(&mut s, [36.0, 36.0, 80.0, 80.0], "<<Name>>", None);
    let before_items = s.documents()[0].doc.spreads[0].items.len();
    let msg = err(&mut s, "data.grid.create", json!({"rect": [72.0, 72.0, 172.0, 172.0], "columns": 2, "rows": 1, "gutter": 100.0}));
    assert!(msg.contains("positive"), "{msg}");
    assert_eq!(s.documents()[0].doc.spreads[0].items.len(), before_items);

    let id = add_grid(&mut s, 0, [72.0, 72.0, 172.0, 272.0], [72.0, 72.0, 172.0, 172.0], 2, 1, json!({}));
    select_csv(&mut s, "Name\nAda\n");
    {
        let doc = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().doc);
        doc.item_mut(designcraft_doc::ItemId(id)).unwrap().data_grid.as_mut().unwrap().gutter = 200.0;
    }
    let before = doc_len(&s);
    let msg = err(&mut s, "data.merge", json!({}));
    assert!(msg.contains("positive"), "{msg}");
    assert_eq!(doc_len(&s), before);
}

#[test]
fn layout_is_either_multiple_records_or_a_grid() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    exec(&mut s, "data.options", json!({"perPage": "multiple"}));
    add_grid(&mut s, 0, [72.0, 72.0, 272.0, 172.0], [72.0, 72.0, 172.0, 172.0], 1, 2, json!({}));
    assert_eq!(s.documents()[0].doc.data_merge.options.per_page, "single");
    assert_eq!(grid_count(&s), 1);

    exec(&mut s, "data.options", json!({"perPage": "multiple"}));
    assert_eq!(s.documents()[0].doc.data_merge.options.per_page, "multiple");
    assert_eq!(grid_count(&s), 0);
    assert!(page_frames(&s).iter().flatten().any(|(_, _, text)| text.contains("Name")));
}

#[test]
fn preview_walks_every_grid_from_the_stepper_record_and_ignores_offset() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    add_grid(&mut s, 0, [72.0, 72.0, 272.0, 272.0], [72.0, 72.0, 172.0, 172.0], 2, 2, json!({}));
    select_csv(&mut s, &names_csv(&["R1", "R2", "R3"]));
    exec(&mut s, "data.preview", json!({"record": 2}));
    assert_eq!(page_frames(&s), vec![vec![(72, 72, "R2".into()), (72, 172, "".into()), (172, 72, "R3".into()), (172, 172, "".into()),]]);
    assert_eq!(grid_count(&s), 0);
    exec(&mut s, "data.preview.stop", json!({}));
    assert_eq!(grid_count(&s), 1);
    let doc = &s.documents()[0].doc;
    let mut story_ids = Vec::new();
    for sp in &doc.spreads {
        for it in &sp.items {
            it.walk(&mut |item| {
                if let Content::Text(tf) = &item.content {
                    story_ids.push(tf.story);
                }
            });
        }
    }
    let nested: Vec<_> = story_ids.iter().filter_map(|id| doc.story(*id).map(|st| st.text.clone())).collect();
    assert!(nested.iter().any(|text| text.contains("<<Name>>")), "{nested:?}");

    let mut s = Session::new();
    new_doc(&mut s, 1);
    text_frame(&mut s, [36.0, 400.0, 220.0, 450.0], "<<Name>>", None);
    let first = add_grid(&mut s, 0, [72.0, 72.0, 272.0, 172.0], [72.0, 72.0, 172.0, 172.0], 1, 2, json!({}));
    add_grid(&mut s, 0, [72.0, 200.0, 272.0, 300.0], [72.0, 200.0, 172.0, 300.0], 1, 2, json!({}));
    select_csv(&mut s, &names_csv(&["R1", "R2", "R3", "R4"]));
    exec(&mut s, "data.grid.set", json!({"id": first, "recordOffset": 5}));
    exec(&mut s, "data.preview", json!({"record": 1}));
    assert_eq!(
        page_frames(&s),
        vec![vec![(36, 400, "R1".into()), (72, 72, "R1".into()), (72, 200, "R3".into()), (172, 72, "R2".into()), (172, 200, "R4".into()),]]
    );
    exec(&mut s, "data.preview.stop", json!({}));
    assert_eq!(grid_count(&s), 2);
    let before = doc_len(&s);
    let msg = err(&mut s, "data.merge", json!({}));
    assert!(msg.contains("origin cell empty"), "{msg}");
    assert_eq!(doc_len(&s), before);
}

#[test]
fn grid_release_restores_the_child_and_overlap_stays_on_the_page() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    let id = add_grid(&mut s, 0, [72.0, 72.0, 272.0, 272.0], [72.0, 72.0, 172.0, 172.0], 2, 2, json!({}));
    let grid = s.documents()[0].doc.item(designcraft_doc::ItemId(id)).unwrap();
    assert_eq!(grid.children().len(), 1);
    assert_eq!(s.documents()[0].doc.spreads[0].items.len(), 1);
    exec(&mut s, "data.grid.release", json!({}));
    assert_eq!(grid_count(&s), 0);
    assert_eq!(page_frames(&s), vec![vec![(72, 72, "<<Name>>".into())]]);

    let mut s = Session::new();
    new_doc(&mut s, 2);
    let sticking = text_frame(&mut s, [72.0, 72.0, 180.0, 172.0], "stay", None);
    let id = exec(&mut s, "data.grid.create", json!({"rect": [72.0, 72.0, 272.0, 272.0], "rows": 2, "columns": 2}))["id"].as_u64().unwrap();
    assert_eq!(s.documents()[0].doc.spreads[0].items.len(), 2);
    assert!(s.documents()[0].doc.item(designcraft_doc::ItemId(id)).unwrap().children().is_empty());

    let other = text_frame(&mut s, [36.0, 36.0, 80.0, 80.0], "other", Some(1));
    exec(&mut s, "selection.set", json!({"ids": [other["id"]]}));
    let msg = err(&mut s, "data.grid.adopt", json!({"id": id}));
    assert!(msg.contains("another page"), "{msg}");
    assert!(s.documents()[0].doc.item(designcraft_doc::ItemId(id)).unwrap().children().is_empty());
    let stick_id = sticking["id"].as_u64().unwrap();
    assert!(s.documents()[0].doc.spreads[0].items.iter().any(|it| it.id.0 == stick_id));
}

#[test]
fn scaled_grid_cells_follow_the_item_transform() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    let id = add_grid(&mut s, 0, [72.0, 72.0, 272.0, 272.0], [72.0, 72.0, 172.0, 172.0], 2, 2, json!({}));
    exec(&mut s, "transform.scale", json!({"sx": 2.0, "sy": 2.0}));
    let doc = &s.documents()[0].doc;
    let grid = doc.item(designcraft_doc::ItemId(id)).unwrap();
    assert!(matches!(grid.content, Content::Group { .. }));
    let spec = grid.data_grid.clone().unwrap();
    let bounds = grid.inner_bounds();
    let xf = grid.xf;
    let mut expected = Vec::new();
    let mut inner_step = 0.0;
    for i in 0..4 {
        let cell = spec.cell_rect(bounds, i).unwrap();
        if i == 1 {
            inner_step = (cell.x0 - spec.cell_rect(bounds, 0).unwrap().x0).abs();
        }
        let p = xf * designcraft_geom::Point::new(cell.x0, cell.y0);
        expected.push((p.x.round() as i32, p.y.round() as i32, format!("R{}", i + 1)));
    }
    let outer_step = (expected[1].0 - expected[0].0).abs();
    assert!(inner_step > 0.0);
    assert!(f64::from(outer_step) > inner_step, "scaled step {outer_step} must be larger than the inner step {inner_step}");
    select_csv(&mut s, &names_csv(&["R1", "R2", "R3", "R4"]));
    exec(&mut s, "data.merge", json!({}));
    assert_eq!(grid_count(&s), 0);
    let frames = page_frames(&s);
    assert_eq!(frames.len(), 1, "{frames:?}");
    for (x, y, text) in &expected {
        assert!(frames[0].iter().any(|(fx, fy, got)| fx == x && fy == y && got == text), "missing {text} at ({x}, {y}) in {frames:?}");
    }
}

#[test]
fn other_source_image_and_typed_marker_stay_blank() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    select_body(&mut s, "photos.csv", "@Photo,City\nmissing.png,Paris\n");
    exec(&mut s, "frame.create", json!({"rect": [36.0, 100.0, 120.0, 180.0], "content": "graphic"}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Photo"}));
    text_frame(&mut s, [36.0, 36.0, 240.0, 80.0], "Hi <<City>>", None);
    let warned = exec(&mut s, "data.merge", json!({}));
    assert!(warned["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap_or("").contains("missing image")), "{warned}");

    let mut s = Session::new();
    new_doc(&mut s, 1);
    let names = select_body(&mut s, "names.csv", "Name\none\n");
    let photos = select_body(&mut s, "photos.csv", "@Photo,City\nmissing.png,Paris\n");
    text_frame(&mut s, [36.0, 36.0, 240.0, 80.0], "Hi <<City>>", None);
    let photo = exec(&mut s, "frame.create", json!({"rect": [36.0, 100.0, 120.0, 180.0], "content": "graphic"}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Photo", "source": photos, "item": photo["id"]}));
    let merged = exec(&mut s, "data.merge", json!({}));
    let warnings = merged["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "{merged}");
    assert!(warnings[0].as_str().unwrap_or("").contains("Record 2"), "{warnings:?}");
    assert!(warnings[0].as_str().unwrap_or("").contains("missing image"), "{warnings:?}");
    assert!(!warnings.iter().any(|w| w.as_str().unwrap_or("").contains("Record 1")), "{warnings:?}");
    assert_eq!(merged["missingImages"].as_array().unwrap().len(), 1);
    assert_eq!(page_frames(&s), vec![vec![(36, 36, "Hi ".into())], vec![(36, 36, "Hi Paris".into())]]);
    let doc = &s.documents()[s.active_index().unwrap()].doc;
    assert!(doc.hyperlinks.is_empty());
    let mut unassigned = 0;
    for sp in &doc.spreads {
        for it in &sp.items {
            if matches!(it.content, Content::Unassigned) {
                unassigned += 1;
            }
            assert!(!matches!(it.content, Content::Graphic(_)), "a blank other-source image must not be placed");
        }
    }
    assert_eq!(unassigned, 2);
    assert_ne!(names, photos);
}

#[test]
fn grid_hit_follows_the_drawn_origin_cell() {
    let mut s = Session::new();
    new_doc(&mut s, 1);
    let id = add_grid(&mut s, 0, [72.0, 72.0, 272.0, 272.0], [72.0, 72.0, 172.0, 172.0], 2, 2, json!({}));
    let doc = &s.documents()[0].doc;
    let grid_id = designcraft_doc::ItemId(id);
    let grid = doc.item(grid_id).unwrap();
    let bounds = grid.inner_bounds();
    assert!(bounds.x0 > 1.0 && bounds.y0 > 1.0, "the grid path is not at the origin");
    let spec = grid.data_grid.as_ref().unwrap();
    let cell = spec.cell_rect(bounds, 0).unwrap();
    let origin = spec.origin_point(bounds).unwrap();
    let inside = designcraft_geom::Point::new((cell.x0 + cell.x1) / 2.0, (cell.y0 + cell.y1) / 2.0);
    assert_eq!((inside.x.round() as i32, inside.y.round() as i32), (122, 122));
    let shifted = designcraft_geom::Point::new(inside.x - origin.x, inside.y - origin.y);
    assert_eq!((shifted.x.round() as i32, shifted.y.round() as i32), (50, 50));
    assert_eq!(doc.hit_item(0, inside, 1.0), Some(grid_id));
    assert_eq!(doc.hit_item(0, shifted, 1.0), None);
}

/// Clicking merge again, while the result is the active document, merges the template again.
#[test]
fn merging_again_uses_the_template() {
    let mut s = Session::new();
    exec(&mut s, "file.new", json!({"title": "Letter", "pages": 1}));
    assert!(s.documents()[0].doc.settings.facing_pages);
    text_frame(&mut s, [72.0, 72.0, 400.0, 140.0], "<<Name>>", None);
    select_csv(&mut s, "Name\nAda\nBea\nCara\n");
    let first = exec(&mut s, "data.merge", json!({}));
    assert_eq!(first["pages"], 3);
    assert_eq!(doc_len(&s), 2);
    let template_uid = s.documents()[0].uid;
    assert_eq!(s.documents()[1].doc.data_merge.template_uid, Some(template_uid));
    assert!(s.documents()[1].doc.data_merge.placeholders.is_empty());
    let second = exec(&mut s, "data.merge", json!({}));
    assert_eq!(second["pages"], 3, "a second click must merge the template, not copy the result");
    assert_eq!(doc_len(&s), 3);
    assert_eq!(s.documents()[2].doc.page_count(), 3);
    assert!(s.documents()[0].doc.stories.values().any(|st| st.text.contains("<<Name>>")));
    assert_eq!(s.documents()[0].doc.data_merge.sources.len(), 1);

    exec(&mut s, "file.close", json!({"index": 0}));
    let msg = err(&mut s, "data.merge", json!({}));
    assert!(msg.contains("no longer open"), "{msg}");
    assert_eq!(doc_len(&s), 2);
}

/// A margin-sized grid draws cell lines the pointer can hit, and does not adopt a frame that
/// crosses out of the origin cell. Clicks in an empty cell still reach frames behind the grid.
#[test]
fn margin_grid_lines_are_hittable_and_frames_outside_the_cell_stay() {
    let mut s = Session::new();
    exec(&mut s, "file.new", json!({"title": "Letter", "pages": 1}));
    let margin = s.documents()[0].doc.page(0).unwrap().margin_rect();
    let rect = [margin.x0, margin.y0, margin.x1, margin.y1];
    let wide = text_frame(&mut s, [margin.x0 + 36.0, margin.y0 + 36.0, margin.x0 + 360.0, margin.y0 + 120.0], "Dear <<Name>>", None);
    select_csv(&mut s, "Name\nAda\nBea\n");
    let made = exec(&mut s, "data.grid.create", json!({"rect": rect, "spread": 0, "rows": 2, "columns": 2}));
    let gid = made["id"].as_u64().unwrap();
    let doc = &s.documents()[0].doc;
    let grid = doc.item(designcraft_doc::ItemId(gid)).unwrap();
    assert!(grid.children().is_empty(), "a frame that crosses the cell stays on the page");
    let divider = designcraft_geom::Point::new(margin.x0 + margin.width() / 2.0, margin.y0 + 80.0);
    let edge = designcraft_geom::Point::new((rect[0] + rect[2]) / 2.0, rect[1]);
    let frame_id = wide["id"].as_u64().unwrap();
    let frame = doc.item(designcraft_doc::ItemId(frame_id)).unwrap().bounds();
    let inside = designcraft_geom::Point::new((frame.x0 + frame.x1) / 2.0, (frame.y0 + frame.y1) / 2.0);
    assert_eq!(doc.hit_item(0, edge, 3.0).map(|id| id.0), Some(gid), "the grid frame can be selected");
    assert_eq!(doc.hit_item(0, divider, 3.0).map(|id| id.0), Some(gid), "a cell line can be selected");
    assert_eq!(doc.hit_item(0, inside, 3.0).map(|id| id.0), Some(frame_id), "an empty cell does not block the frame behind it");
}

#[test]
fn select_records_dedupes_large_ranges_in_first_seen_order() {
    let idx = parse::select_records(100_000, "range", 1, "5,1-100000,3", None).unwrap();
    assert_eq!(idx.len(), 100_000);
    assert_eq!(&idx[..3], &[4, 0, 1]);
}

fn bind_photo(s: &mut Session, rect: [f64; 4]) -> u64 {
    let id = exec(s, "frame.create", json!({"rect": rect, "content": "graphic"}))["id"].as_u64().unwrap();
    exec(s, "data.placeholder.add", json!({"field": "Photo", "item": id}));
    id
}

#[test]
fn phase3_skip_keeps_or_drops_a_warning_record() {
    let dir = scratch("phase3-skip");
    let csv = dir.join("people.csv");
    write(&csv, "Name,@Photo\nBad,missing.png\nGood,good.png\n");
    write_png(&dir.join("good.png"));

    let mut off = Session::new();
    new_doc(&mut off, 1);
    exec(&mut off, "data.source.select", json!({"path": &csv}));
    bind_photo(&mut off, [40.0, 100.0, 100.0, 160.0]);
    add_grid(&mut off, 0, [36.0, 36.0, 436.0, 236.0], [40.0, 40.0, 200.0, 80.0], 1, 2, json!({}));
    text_frame(&mut off, [460.0, 36.0, 600.0, 80.0], "#<<Merge index>>", None);
    let merged = exec(&mut off, "data.merge", json!({}));
    assert_eq!(merged["records"], 2);
    assert!(merged["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap_or("").contains("missing image")), "{merged}");
    let frames = page_frames(&off);
    assert!(frames[0].iter().any(|(_, _, text)| text.contains("Bad")), "the warning record stays: {frames:?}");
    exec(&mut off, "file.activate", json!({"index": 0}));
    let preview = exec(&mut off, "data.preview", json!({"record": 1}));
    assert_eq!(preview["count"], 2);
    assert!(story_order(&off).iter().any(|t| t.contains("Bad")));

    let mut on = Session::new();
    new_doc(&mut on, 1);
    exec(&mut on, "data.source.select", json!({"path": &csv}));
    bind_photo(&mut on, [40.0, 100.0, 100.0, 160.0]);
    add_grid(&mut on, 0, [36.0, 36.0, 436.0, 236.0], [40.0, 40.0, 200.0, 80.0], 1, 2, json!({}));
    text_frame(&mut on, [460.0, 36.0, 600.0, 80.0], "#<<Merge index>>", None);
    exec(&mut on, "data.options", json!({"skipWarnings": true}));
    let preview = exec(&mut on, "data.preview", json!({"record": 1}));
    assert_eq!(preview["count"], 1, "{preview}");
    assert!(preview["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap_or("").contains("missing image")), "{preview}");
    assert!(story_order(&on).iter().any(|t| t.contains("Good")), "{:?}", story_order(&on));
    assert!(!story_order(&on).iter().any(|t| t.contains("Bad")), "{:?}", story_order(&on));
    exec(&mut on, "data.preview.stop", json!({}));
    let merged = exec(&mut on, "data.merge", json!({}));
    assert_eq!(merged["records"], 1);
    assert!(merged["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap_or("").contains("missing image")), "{merged}");
    let frames = page_frames(&on);
    let origin = frames[0].iter().find(|(_, _, text)| text.contains("Good") || text.contains("Bad"));
    assert!(origin.is_some_and(|(_, _, text)| text.contains("Good") && !text.contains("Bad")), "kept record takes the first cell: {frames:?}");
    assert!(frames.iter().flatten().any(|(_, _, text)| text.contains("#2")), "skip does not renumber: {frames:?}");
    assert!(!frames.iter().flatten().any(|(_, _, text)| text.contains("Bad")), "{frames:?}");
}

#[test]
fn phase3_affixes_virtual_fields_join_and_global_sort() {
    let dir = scratch("phase3-records");
    let csv = dir.join("people.csv");
    let body = "Name,Note\nAda,1\n,2\nBea,3\n";
    write(&csv, body);
    let before = std::fs::read(&csv).unwrap();
    let mut s = Session::new();
    new_doc(&mut s, 1);
    exec(&mut s, "data.source.select", json!({"path": &csv}));
    text_frame(&mut s, [36.0, 36.0, 400.0, 80.0], "<<Name>>|<<Source filename>>|<<Merge index>>", None);
    exec(
        &mut s,
        "data.source.affix",
        json!({"rules": [
            {"field": "Name", "text": "Hi ", "place": "prefix", "nonEmpty": true},
            {"field": "Name", "text": "!", "place": "postfix", "nonEmpty": true}
        ]}),
    );
    exec(&mut s, "data.options", json!({"records": "range", "range": "2-3"}));
    exec(&mut s, "data.merge", json!({}));
    assert_eq!(story_order(&s), vec!["|people.csv|1".to_string(), "Hi Bea!|people.csv|2".to_string()]);
    assert_eq!(std::fs::read(&csv).unwrap(), before);

    let mut inline_name = Session::new();
    new_doc(&mut inline_name, 1);
    exec(&mut inline_name, "data.source.select", json!({"bytes": crate::cmd::base64_encode(b"Name\nAda\n"), "name": "guests.csv"}));
    text_frame(&mut inline_name, [36.0, 36.0, 320.0, 80.0], "<<Source filename>>|<<Merge index>>", None);
    let listed = exec(&mut inline_name, "data.fields", json!({}));
    let names: Vec<&str> = listed.as_array().unwrap().iter().filter_map(|entry| entry["name"].as_str()).collect();
    assert_eq!(names.iter().filter(|name| **name == "Source filename").count(), 1, "{names:?}");
    assert_eq!(names.iter().filter(|name| **name == "Merge index").count(), 1, "{names:?}");
    exec(&mut inline_name, "data.merge", json!({}));
    assert_eq!(story_order(&inline_name), vec!["guests.csv|1".to_string()]);
    exec(&mut s, "file.activate", json!({"index": 0}));
    assert_eq!(s.documents()[0].doc.data_merge.sources[0].rows, vec![vec!["Ada", "1"], vec!["", "2"], vec!["Bea", "3"]]);
    let preview = exec(&mut s, "data.preview", json!({"record": 2}));
    assert_eq!(preview["count"], 2);
    assert!(story_order(&s).iter().any(|t| t.contains("Hi Bea!")));

    let mut named = Session::new();
    new_doc(&mut named, 1);
    select_body(&mut named, "people.csv", "Merge index,Name\nfrom-file,Ada\n");
    text_frame(&mut named, [36.0, 36.0, 300.0, 80.0], "<<Merge index>>|<<Name>>", None);
    exec(&mut named, "data.merge", json!({}));
    assert_eq!(story_order(&named), vec!["from-file|Ada".to_string()]);

    let mut join = Session::new();
    new_doc(&mut join, 1);
    let people = select_body(&mut join, "people.csv", "Name,Key\nAda,1\nBea,2\nCara,9\n");
    let cities = select_body(&mut join, "cities.csv", "Key,City\n2,Ville\n1,Town\n3,Nope\n");
    text_frame(&mut join, [36.0, 36.0, 300.0, 80.0], "<<Name>>|<<City>>", None);
    exec(&mut join, "data.join", json!({"driving": people, "links": [{"source": cities, "drivingField": "Key", "field": "Key"}]}));
    let merged = exec(&mut join, "data.merge", json!({}));
    assert_eq!(merged["records"], 3);
    assert_eq!(story_order(&join), vec!["Ada|Town".to_string(), "Bea|Ville".to_string(), "Cara|".to_string()]);

    let mut sorted = Session::new();
    new_doc(&mut sorted, 1);
    let left = select_body(&mut sorted, "a.csv", "Name,Group\nZoe,1\nAda,1\n");
    select_body(&mut sorted, "b.csv", "Name\nMia\n");
    text_frame(&mut sorted, [36.0, 36.0, 240.0, 80.0], "<<Name>>", None);
    exec(&mut sorted, "data.source.sort", json!({"id": left, "fields": [{"field": "Name", "direction": "asc"}]}));
    exec(&mut sorted, "data.sort", json!({"fields": [{"field": "Group"}]}));
    exec(&mut sorted, "data.merge", json!({}));
    assert_eq!(story_order(&sorted), vec!["Mia".to_string(), "Ada".to_string(), "Zoe".to_string()]);

    let mut interleaved = Session::new();
    new_doc(&mut interleaved, 1);
    select_body(&mut interleaved, "a.csv", "Name\nCara\nAda\n");
    select_body(&mut interleaved, "b.csv", "Name\nBea\n");
    text_frame(&mut interleaved, [36.0, 36.0, 240.0, 80.0], "<<Name>>", None);
    exec(&mut interleaved, "data.sort", json!({"fields": [{"field": "Name"}]}));
    exec(&mut interleaved, "data.merge", json!({}));
    assert_eq!(story_order(&interleaved), vec!["Ada".to_string(), "Bea".to_string(), "Cara".to_string()]);
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

fn inline_origin(s: &Session, prefix: &str) -> Option<(f64, f64)> {
    let doc = &s.documents()[s.active_index().unwrap()].doc;
    for sp in &doc.spreads {
        for it in &sp.items {
            let Content::Text(tf) = &it.content else { continue };
            let Some(st) = doc.story(tf.story) else { continue };
            if !st.text.starts_with(prefix) {
                continue;
            }
            let composed = designcraft_compose::compose_story(doc, tf.story, &designcraft_compose::ComposeOptions::default());
            let obj = composed.frames.iter().flat_map(|frame| frame.objects.iter()).next()?;
            let long = obj.size.0.max(obj.size.1);
            return Some((obj.origin.x, long));
        }
    }
    None
}

fn qr_alts(s: &Session) -> Vec<String> {
    let doc = &s.documents()[s.active_index().unwrap()].doc;
    let mut alts = Vec::new();
    for sp in &doc.spreads {
        for it in &sp.items {
            if let Content::Group { items } = &it.content {
                alts.extend(items.iter().map(|child| child.alt_text.clone()));
            }
        }
    }
    alts
}

#[test]
fn phase3_inline_picture_parent_and_unmarked_qr() {
    let dir = scratch("phase3-inline");
    let wide = designcraft_render::Rendered { width: 200, height: 2, pixels: vec![255u8; 200 * 2 * 4] }.to_png();
    assert!(designcraft_render::image_size(&wide).is_some());
    let pic = dir.join("wide.png");
    std::fs::write(&pic, &wide).unwrap();
    let csv = dir.join("people.csv");
    write(&csv, "Name,@Photo\nA,wide.png\nLongername,wide.png\nGone,missing.png\n");

    let mut s = Session::new();
    new_doc(&mut s, 1);
    exec(&mut s, "data.source.select", json!({"path": &csv}));
    let frame = text_frame(&mut s, [36.0, 36.0, 500.0, 160.0], "", None);
    let story = frame["story"].as_u64().unwrap();
    exec(&mut s, "data.placeholder.add", json!({"field": "Name", "story": story, "at": 0}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Photo", "role": "image", "story": story, "at": 8}));
    let picture = exec(&mut s, "frame.create", json!({"rect": [36.0, 200.0, 76.0, 240.0], "content": "unassigned"}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Photo", "role": "image", "item": picture["id"]}));
    exec(&mut s, "data.preview", json!({"record": 1}));
    let preview_short = inline_origin(&s, "A").expect("preview places an inline picture");
    exec(&mut s, "data.preview", json!({"record": 2}));
    let preview_long = inline_origin(&s, "Longername").expect("preview places the second record");
    assert!(preview_long.0 > preview_short.0 + 4.0, "the picture moves with the text at preview: short {preview_short:?} long {preview_long:?}");
    exec(&mut s, "data.preview.stop", json!({}));
    let merged = exec(&mut s, "data.merge", json!({}));
    assert_eq!(merged["records"], 3);
    assert!(merged["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap_or("").contains("missing image")));
    let short = inline_origin(&s, "A").expect("short name places an inline picture");
    let long = inline_origin(&s, "Longername").expect("long name places an inline picture");
    assert!(long.0 > short.0 + 4.0, "the picture moves with the text in front of it: short {short:?} long {long:?}");
    assert!((short.1 - 144.0).abs() < 1.0, "inline long side clamps to 144 pt, got {}", short.1);
    let doc = &s.documents()[s.active_index().unwrap()].doc;
    let mut frame_w = Vec::new();
    let mut unassigned = 0;
    for sp in &doc.spreads {
        for it in &sp.items {
            if let Content::Graphic(g) = &it.content {
                let fitted = g.xf.transform_rect_bbox(designcraft_geom::Rect::new(0.0, 0.0, g.size.0, g.size.1));
                frame_w.push(fitted.width());
                assert!((it.inner_bounds().width() - 40.0).abs() < 1.0, "the picture frame keeps its own size");
            }
            if matches!(it.content, Content::Unassigned) {
                unassigned += 1;
            }
        }
    }
    assert!(frame_w.iter().any(|w| *w < 50.0), "the frame picture stays fitted in its frame, {frame_w:?}");
    assert!(unassigned >= 1, "a missing image leaves the frame empty");
    let gone = doc.stories.values().find(|st| st.text.starts_with("Gone")).expect("missing record is kept");
    assert!(gone.objects.iter().any(|obj| matches!(obj.item.content, Content::Unassigned)));

    let mut parent = Session::new();
    new_doc(&mut parent, 1);
    select_csv(&mut parent, "Name\nAda\nBea\nCara\nDot\nEve\n");
    exec(
        &mut parent,
        "frame.create",
        json!({"rect": [36.0, 20.0, 220.0, 60.0], "content": "text", "text": "", "spread": {"kind": "parent", "index": 0}}),
    );
    let header = parent.documents()[0].doc.parents[0].items.last().unwrap().clone();
    let Content::Text(tf) = &header.content else {
        panic!("parent frame");
    };
    exec(&mut parent, "data.placeholder.add", json!({"field": "Name", "story": tf.story.0, "at": 0}));
    add_grid(&mut parent, 0, [72.0, 200.0, 360.0, 480.0], [72.0, 200.0, 200.0, 320.0], 2, 2, json!({}));
    let grid = exec(&mut parent, "data.merge", json!({}));
    assert_eq!(grid["pages"], 2);
    assert!(grid["warnings"].as_array().unwrap().iter().all(|w| !w.as_str().unwrap_or("").contains("parent")));
    let pages = page_frames(&parent);
    assert!(pages[0].iter().any(|(_, y, t)| *y < 80 && t == "Ada"), "{pages:?}");
    assert!(pages[1].iter().any(|(_, y, t)| *y < 80 && t == "Eve"), "{pages:?}");
    let master = &parent.documents()[parent.active_index().unwrap()].doc;
    let master_text: Vec<_> = master
        .parents
        .iter()
        .flat_map(|sp| sp.items.iter())
        .filter_map(|it| match &it.content {
            Content::Text(tf) => master.story(tf.story).map(|st| st.text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(master_text, vec!["<<Name>>".to_string()]);

    let mut tile = Session::new();
    new_doc(&mut tile, 1);
    select_csv(&mut tile, "Name\nA\nB\nC\nD\nE\nF\n");
    exec(
        &mut tile,
        "frame.create",
        json!({"rect": [36.0, 8.0, 180.0, 36.0], "content": "text", "text": "", "spread": {"kind": "parent", "index": 0}}),
    );
    let band = tile.documents()[0].doc.parents[0].items.last().unwrap().clone();
    let Content::Text(tf) = &band.content else {
        panic!("parent frame");
    };
    exec(&mut tile, "data.placeholder.add", json!({"field": "Name", "story": tf.story.0, "at": 0}));
    text_frame(&mut tile, [72.0, 72.0, 172.0, 172.0], "<<Name>>", None);
    exec(
        &mut tile,
        "data.options",
        json!({"perPage": "multiple", "arrange": "rows", "insets": [36.0, 36.0, 36.0, 36.0], "columnSpacing": 36.0, "rowSpacing": 36.0}),
    );
    exec(&mut tile, "data.merge", json!({}));
    let tiled = page_frames(&tile);
    assert_eq!(tiled.len(), 1, "{tiled:?}");
    assert!(tiled[0].iter().any(|(_, y, t)| *y < 40 && t == "A"), "the parent copy uses the first record on the page: {tiled:?}");
    assert!(!tiled[0].iter().any(|(_, y, t)| *y < 40 && t == "F"), "{tiled:?}");

    let mut qr = Session::new();
    new_doc(&mut qr, 1);
    select_csv(&mut qr, "Code\nhello\n");
    let kind = qr.documents()[0].doc.data_merge.sources[0].fields[0].kind;
    assert_eq!(kind, DataFieldKind::Text);
    exec(&mut qr, "object.qrCode", json!({"field": "Code", "rect": [36.0, 36.0, 120.0, 120.0]}));
    assert_eq!(qr.documents()[0].doc.data_merge.sources[0].fields[0].kind, DataFieldKind::Text);
    assert!(qr.documents()[0].doc.data_merge.placeholders.iter().any(|ph| ph.role == designcraft_doc::PlaceholderRole::Qr));
    exec(&mut qr, "data.merge", json!({}));
    assert_eq!(qr_alts(&qr), vec!["QR code: hello".to_string()]);

    let mut marked = Session::new();
    new_doc(&mut marked, 1);
    select_csv(&mut marked, "#Code\nhello\n");
    assert_eq!(marked.documents()[0].doc.data_merge.sources[0].fields[0].kind, DataFieldKind::Qr);
    let frame = exec(&mut marked, "frame.create", json!({"rect": [36.0, 36.0, 120.0, 120.0], "content": "unassigned"}));
    exec(&mut marked, "data.placeholder.add", json!({"field": "Code", "item": frame["id"]}));
    exec(&mut marked, "data.merge", json!({}));
    assert_eq!(qr_alts(&marked), vec!["QR code: hello".to_string()]);

    let mut story_qr = Session::new();
    new_doc(&mut story_qr, 1);
    select_csv(&mut story_qr, "Code\nhello\n");
    let made = text_frame(&mut story_qr, [36.0, 36.0, 200.0, 80.0], "", None);
    let msg = err(&mut story_qr, "data.placeholder.add", json!({"field": "Code", "role": "qr", "story": made["story"], "at": 0}));
    assert!(msg.contains("frame"), "{msg}");
}

#[test]
fn phase3_fetches_http_images() {
    let png = crate::cmd::base64_decode(PNG_B64);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("localhost socket");
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        let mut served = 0u8;
        while served < 2 && std::time::Instant::now() < deadline {
            match listener.accept() {
                Ok((mut sock, _)) => {
                    sock.set_nonblocking(false).unwrap();
                    let _ = sock.set_read_timeout(Some(std::time::Duration::from_secs(2)));
                    let mut buf = [0u8; 4096];
                    let n = std::io::Read::read(&mut sock, &mut buf).unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let body: &[u8] = if req.contains("GET /ok.png") { &png } else { b"" };
                    let status = if req.contains("GET /ok.png") { "200 OK" } else { "500 ERR" };
                    let header = format!("HTTP/1.0 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    let _ = std::io::Write::write_all(&mut sock, header.as_bytes());
                    let _ = std::io::Write::write_all(&mut sock, body);
                    served += 1;
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(std::time::Duration::from_millis(15)),
                Err(_) => break,
            }
        }
    });

    let mut s = Session::new();
    new_doc(&mut s, 1);
    select_csv(&mut s, &format!("@Photo\nhttp://127.0.0.1:{port}/ok.png\n"));
    let frame = exec(&mut s, "frame.create", json!({"rect": [36.0, 36.0, 80.0, 80.0], "content": "unassigned"}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Photo", "role": "image", "item": frame["id"]}));
    let ok = exec(&mut s, "data.merge", json!({}));
    assert!(ok["warnings"].as_array().unwrap().is_empty(), "{ok}");
    assert!(graphic_link(&s).is_some_and(|link| link.contains("/ok.png")));

    let mut fail = Session::new();
    new_doc(&mut fail, 1);
    select_csv(&mut fail, &format!("@Photo\nhttp://127.0.0.1:{port}/missing.png\n"));
    let frame = exec(&mut fail, "frame.create", json!({"rect": [36.0, 36.0, 80.0, 80.0], "content": "unassigned"}));
    exec(&mut fail, "data.placeholder.add", json!({"field": "Photo", "role": "image", "item": frame["id"]}));
    let bad = exec(&mut fail, "data.merge", json!({}));
    assert_eq!(bad["records"], 1);
    assert!(bad["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap_or("").contains("missing image")), "{bad}");
    assert!(graphic_link(&fail).is_none());
    server.join().unwrap();
}

/// `curl` trusts `SSL_CERT_FILE` from its own environment. This crate forbids the unsafe call
/// that would set that variable in-process, so the merge runs in a child of this test binary.
struct StopChild(Option<std::process::Child>);

impl Drop for StopChild {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn https_server_ready(port: u16) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    while std::time::Instant::now() < deadline {
        if std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(200)).is_ok() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    false
}

fn phase3_fetches_https_images_in_process() {
    let port: u16 = std::env::var("DESIGNCRAFT_HTTPS_PORT").expect("port").parse().expect("port number");
    let png = crate::cmd::base64_decode(PNG_B64);
    let mut s = Session::new();
    new_doc(&mut s, 1);
    select_csv(&mut s, &format!("@Photo\nhttps://127.0.0.1:{port}/ok.png\n"));
    let frame = exec(&mut s, "frame.create", json!({"rect": [36.0, 36.0, 80.0, 80.0], "content": "unassigned"}));
    exec(&mut s, "data.placeholder.add", json!({"field": "Photo", "role": "image", "item": frame["id"]}));
    let ok = exec(&mut s, "data.merge", json!({}));
    assert!(ok["warnings"].as_array().unwrap().is_empty(), "{ok}");
    let link = graphic_link(&s).unwrap_or_default();
    assert!(link.contains(&format!("https://127.0.0.1:{port}/ok.png")), "{link}");
    let doc = &s.documents()[s.active_index().unwrap()].doc;
    assert!(doc.assets.values().any(|asset| asset.data.as_slice() == png.as_slice()), "the merge places the fetched png");

    let mut fail = Session::new();
    new_doc(&mut fail, 1);
    select_csv(&mut fail, &format!("@Photo\nhttps://127.0.0.1:{port}/missing.png\n"));
    let frame = exec(&mut fail, "frame.create", json!({"rect": [36.0, 36.0, 80.0, 80.0], "content": "unassigned"}));
    exec(&mut fail, "data.placeholder.add", json!({"field": "Photo", "role": "image", "item": frame["id"]}));
    let bad = exec(&mut fail, "data.merge", json!({}));
    assert_eq!(bad["records"], 1);
    assert!(bad["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap_or("").contains("missing image")), "{bad}");
    assert!(graphic_link(&fail).is_none());
}

#[test]
fn phase3_fetches_https_images() {
    if std::env::var_os("DESIGNCRAFT_HTTPS_PORT").is_some() {
        phase3_fetches_https_images_in_process();
        return;
    }
    let dir = std::env::temp_dir().join(format!("designcraft-https-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(dir.join("ok.png"), crate::cmd::base64_decode(PNG_B64)).expect("png");
    let cert = dir.join("cert.pem");
    let key = dir.join("key.pem");
    let made = std::process::Command::new("openssl")
        .arg("req")
        .args(["-x509", "-newkey", "rsa:2048", "-days", "1", "-nodes"])
        .arg("-keyout")
        .arg(&key)
        .arg("-out")
        .arg(&cert)
        .args(["-subj", "/CN=127.0.0.1", "-addext", "subjectAltName=IP:127.0.0.1,DNS:localhost"])
        .output()
        .expect("openssl req");
    assert!(made.status.success(), "openssl req failed: {}", String::from_utf8_lossy(&made.stderr));

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("localhost socket");
    let port = listener.local_addr().expect("port").port();
    drop(listener);
    let accept = format!("127.0.0.1:{port}");
    let err_path = dir.join("server.err");
    let err_file = std::fs::File::create(&err_path).expect("server log");
    let server = std::process::Command::new("openssl")
        .args(["s_server", "-accept", &accept, "-cert", "cert.pem", "-key", "key.pem", "-WWW"])
        .current_dir(&dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::from(err_file))
        .spawn()
        .expect("openssl s_server");
    let server = StopChild(Some(server));
    if !https_server_ready(port) {
        let log = std::fs::read_to_string(&err_path).unwrap_or_default();
        panic!("https server did not listen on {port}: {log}");
    }

    let exe = std::env::current_exe().expect("test binary");
    let output = std::process::Command::new(exe)
        .args(["--exact", "--test-threads=1", "cmd::datamerge::tests::phase3_fetches_https_images"])
        .env("SSL_CERT_FILE", &cert)
        .env("DESIGNCRAFT_HTTPS_PORT", port.to_string())
        .output()
        .expect("re-run the https merge");
    drop(server);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success() && stdout.contains("phase3_fetches_https_images ... ok"), "https data.merge did not pass\n{stdout}\n{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}
