//! Excel workbooks (`.xlsx`) → a story holding one table: the first worksheet's used range, cell
//! values as shown text (shared and inline strings, numbers, booleans). Data merge uses
//! [`records`], which keeps empty cells and can name a sheet.

use std::collections::BTreeMap;

use designcraft_doc::{CharFormat, ParaFormat, Story, StoryId, Table};

use crate::docx::{El, parse, part};
use crate::{ImportError, Imported};

/// "B12" → (row 11, column 1).
fn cell_ref(r: &str) -> Option<(usize, usize)> {
    let letters: String = r.chars().take_while(char::is_ascii_alphabetic).collect();
    let row: usize = r[letters.len()..].parse().ok()?;
    let col = letters.chars().try_fold(0usize, |acc, c| {
        let digit = (c.to_ascii_uppercase() as u8).checked_sub(b'A')? as usize + 1;
        acc.checked_mul(26)?.checked_add(digit)
    })?;
    Some((row.checked_sub(1)?, col.checked_sub(1)?))
}

/// The worksheet part of the first sheet (workbook order), via the workbook relationships.
fn first_sheet(zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>) -> Result<Option<String>, ImportError> {
    let Some(wb) = part(zip, "xl/workbook.xml")? else { return Ok(None) };
    let Some(rels) = part(zip, "xl/_rels/workbook.xml.rels")? else { return Ok(None) };
    Ok(first_sheet_target(&wb, &rels))
}

fn first_sheet_target(wb: &str, rels: &str) -> Option<String> {
    let wb = parse(wb).ok()?;
    let rid = wb.child("sheets")?.els().find(|e| e.name == "sheet")?.attr("id")?.to_string();
    let rels = parse(rels).ok()?;
    let target = rels.els().find(|r| r.attr("Id") == Some(rid.as_str()))?.attr("Target")?.trim_start_matches('/').to_string();
    Some(if target.starts_with("xl/") { target } else { format!("xl/{target}") })
}

/// The text of a shared string (`<si>`) or inline string (`<is>`): its own `<t>` and the `<t>` of its
/// `<r>` runs. Phonetic guides (`<rPh>`, `<phoneticPr>`) are the reading, not the cell text.
fn string_item(si: &El) -> String {
    si.els()
        .map(|e| match e.name.as_str() {
            "t" => e.text(),
            "r" => e.els().filter(|t| t.name == "t").map(El::text).collect(),
            _ => String::new(),
        })
        .collect()
}

fn value(c: &El, shared: &[String]) -> String {
    let v = || c.child("v").map(El::text).unwrap_or_default();
    match c.attr("t") {
        Some("s") => v().trim().parse::<usize>().ok().and_then(|i| shared.get(i).cloned()).unwrap_or_default(),
        Some("inlineStr") => c.child("is").map(string_item).unwrap_or_default(),
        // A cached formula string is text even when it looks like a number.
        Some("str") => v(),
        Some("b") => {
            if v().trim() == "1" {
                "TRUE".into()
            } else {
                "FALSE".into()
            }
        }
        _ => {
            let raw = v();
            match raw.trim().parse::<f64>() {
                Ok(f) if f.is_finite() => general(f),
                _ => raw,
            }
        }
    }
}

/// A number as Excel's General format shows it: at most 15 significant digits (Excel stores
/// doubles with 17, so `=0.1+0.2` is cached as 0.30000000000000004 and shown as 0.3), whole
/// numbers without ".0", and an exponent for very large or very small values.
fn general(f: f64) -> String {
    let r = format!("{f:.14e}").parse::<f64>().unwrap_or(f);
    let a = r.abs();
    if r == 0.0 {
        "0".into()
    } else if (1e-9..1e15).contains(&a) {
        // Shortest text that reads back as the rounded value; whole numbers drop the ".0".
        r.to_string()
    } else {
        let s = format!("{r:E}");
        match s.split_once('E') {
            Some((m, e)) if !e.starts_with('-') => format!("{m}E+{e}"),
            _ => s,
        }
    }
}

/// The first and last column used on the header row, if that row has any cells.
fn header_span(cells: &[(usize, usize, String)], header_row: usize) -> Option<(usize, usize)> {
    cells.iter().filter(|(r, _, _)| *r == header_row).fold(None, |span, (_, c, _)| match span {
        None => Some((*c, *c)),
        Some((lo, hi)) => Some((lo.min(*c), hi.max(*c))),
    })
}

/// One row's values laid into `width` columns starting at column `min_c`; values outside that
/// span come back separately (only the non-empty ones), so the caller can warn.
fn place_row(cols: &[(usize, String)], min_c: usize, width: usize) -> (Vec<String>, Vec<String>) {
    let mut row = vec![String::new(); width];
    let mut extra: Vec<String> = Vec::new();
    for (c, v) in cols {
        let at = c.checked_sub(min_c).filter(|at| *at < width);
        match at {
            Some(at) => {
                if let Some(cell) = row.get_mut(at) {
                    *cell = v.clone();
                }
            }
            None if !v.is_empty() => extra.push(v.clone()),
            None => {}
        }
    }
    (row, extra)
}

/// One worksheet as a grid of cached cell text, empty cells kept, for data merge.
/// The first used row is the header. Later rows that are entirely empty are left out.
/// Cells past the header are kept so the caller can warn. Over 200 columns or 100,000
/// data rows is an error and the grid is not built.
pub fn records(bytes: &[u8], sheet: Option<&str>) -> Result<Vec<Vec<String>>, ImportError> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| ImportError::Corrupt(e.to_string()))?;
    let sheet_part = sheet_target(&mut zip, sheet)?;
    let sheet_xml = parse(&part(&mut zip, &sheet_part)?.ok_or_else(|| ImportError::Corrupt(format!("no {sheet_part} (not an Excel workbook)")))?)?;
    let shared: Vec<String> = part(&mut zip, "xl/sharedStrings.xml")?
        .map(|x| parse(&x))
        .transpose()?
        .map(|sst| sst.els().filter(|e| e.name == "si").map(string_item).collect())
        .unwrap_or_default();
    let mut cells: Vec<(usize, usize, String)> = Vec::new();
    if let Some(data) = sheet_xml.child("sheetData") {
        for (ri, row) in data.els().filter(|e| e.name == "row").enumerate() {
            for (ci, c) in row.els().filter(|e| e.name == "c").enumerate() {
                let (r, col) = c.attr("r").and_then(cell_ref).unwrap_or((ri, ci));
                cells.push((r, col, value(c, &shared)));
            }
        }
    }
    if cells.is_empty() {
        return Err(ImportError::Corrupt("the worksheet is empty".into()));
    }
    let header_row = cells.iter().map(|c| c.0).min().unwrap_or(0);
    let Some((min_c, max_c)) = header_span(&cells, header_row) else {
        return Err(ImportError::Corrupt("the worksheet has no header".into()));
    };
    let width = max_c.saturating_sub(min_c).saturating_add(1);
    if width > crate::DATA_MERGE_MAX_COLS {
        return Err(ImportError::Corrupt(format!("the sheet has {width} columns; the limit is {}", crate::DATA_MERGE_MAX_COLS)));
    }
    // Group by row. Gaps (empty rows) are not materialised.
    let mut by_row: BTreeMap<usize, Vec<(usize, String)>> = BTreeMap::new();
    for (r, c, v) in cells {
        by_row.entry(r).or_default().push((c, v));
    }
    let mut data_rows = 0usize;
    for (r, cols) in &by_row {
        if *r == header_row {
            continue;
        }
        let any = cols.iter().any(|(_, v)| !v.is_empty());
        if any {
            data_rows = data_rows.saturating_add(1);
        }
    }
    if data_rows > crate::DATA_MERGE_MAX_ROWS {
        return Err(ImportError::Corrupt(format!("the sheet has {data_rows} data rows; the limit is {}", crate::DATA_MERGE_MAX_ROWS)));
    }
    let mut out = Vec::with_capacity(data_rows.saturating_add(1));
    for (r, cols) in &by_row {
        let (mut row, extra) = place_row(cols, min_c, width);
        if *r != header_row && row.iter().all(String::is_empty) && extra.is_empty() {
            continue;
        }
        row.extend(extra);
        out.push(row);
    }
    Ok(out)
}

/// Workbook order, or the sheet named `wanted`.
fn sheet_target(zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, wanted: Option<&str>) -> Result<String, ImportError> {
    let wb = parse(&part(zip, "xl/workbook.xml")?.ok_or_else(|| ImportError::Corrupt("no xl/workbook.xml (not an Excel workbook)".into()))?)?;
    let sheets: Vec<(String, String)> = wb
        .child("sheets")
        .map(|s| s.els().filter(|e| e.name == "sheet").filter_map(|e| Some((e.attr("name")?.to_string(), e.attr("id")?.to_string()))).collect())
        .unwrap_or_default();
    let (name, rid) = match wanted {
        None => sheets.first().cloned().ok_or_else(|| ImportError::Corrupt("the workbook has no sheets".into()))?,
        Some(w) => sheets.into_iter().find(|(n, _)| n == w).ok_or_else(|| ImportError::Corrupt(format!("no worksheet \"{w}\"")))?,
    };
    let rels = parse(&part(zip, "xl/_rels/workbook.xml.rels")?.ok_or_else(|| ImportError::Corrupt("no workbook relationships".into()))?)?;
    let target = rels
        .els()
        .find(|r| r.attr("Id") == Some(rid.as_str()))
        .and_then(|r| r.attr("Target"))
        .ok_or_else(|| ImportError::Corrupt(format!("worksheet \"{name}\" has no file")))?;
    let target = target.trim_start_matches('/');
    Ok(if target.starts_with("xl/") { target.to_string() } else { format!("xl/{target}") })
}

pub fn import(bytes: &[u8]) -> Result<Imported, ImportError> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| ImportError::Corrupt(e.to_string()))?;
    let sheet_part = first_sheet(&mut zip)?.unwrap_or_else(|| "xl/worksheets/sheet1.xml".into());
    let sheet = parse(&part(&mut zip, &sheet_part)?.ok_or_else(|| ImportError::Corrupt(format!("no {sheet_part} (not an Excel workbook)")))?)?;
    let shared: Vec<String> = part(&mut zip, "xl/sharedStrings.xml")?
        .map(|x| parse(&x))
        .transpose()?
        .map(|sst| sst.els().filter(|e| e.name == "si").map(string_item).collect())
        .unwrap_or_default();
    // A `t="s"` cell holds an index into the shared strings and each one is materialised as its
    // own `String`, so one large shared string referenced by many cells multiplies: a 1 MB string
    // and 3M references is terabytes, allocated before the grid cap below ever applies, and an
    // allocation that large aborts the process rather than failing (#1584). Collection carries a
    // byte budget and a cell count of its own.
    const MAX_TEXT_BYTES: usize = 64 << 20;
    const MAX_COLLECTED_CELLS: usize = 400_000;
    let mut cells: Vec<((usize, usize), String)> = Vec::new();
    let mut text_bytes = 0usize;
    let mut cut = false;
    if let Some(data) = sheet.child("sheetData") {
        'rows: for (ri, row) in data.els().filter(|e| e.name == "row").enumerate() {
            for (ci, c) in row.els().filter(|e| e.name == "c").enumerate() {
                let at = c.attr("r").and_then(cell_ref).unwrap_or((ri, ci));
                let v = value(c, &shared);
                if v.is_empty() {
                    continue;
                }
                if cells.len() >= MAX_COLLECTED_CELLS || text_bytes.saturating_add(v.len()) > MAX_TEXT_BYTES {
                    cut = true;
                    break 'rows;
                }
                text_bytes = text_bytes.saturating_add(v.len());
                cells.push((at, v));
            }
        }
    }
    let mut warnings = Vec::new();
    if cut {
        warnings.push(format!(
            "the worksheet holds more than {} cells or {} MB of text: the rest was not read",
            MAX_COLLECTED_CELLS,
            MAX_TEXT_BYTES / (1 << 20)
        ));
    }
    if cells.is_empty() {
        warnings.push("the worksheet is empty".into());
    }
    // The used range, from the first used row/column. Cell references come from the file: the
    // table is capped before anything is allocated for it.
    const MAX_CELLS: usize = 100_000;
    const MAX_COLS: usize = 1_000;
    let (r0, c0) = cells.iter().fold((usize::MAX, usize::MAX), |(r, c), ((a, b), _)| (r.min(*a), c.min(*b)));
    let (r1, c1) = cells.iter().fold((0, 0), |(r, c), ((a, b), _)| (r.max(*a), c.max(*b)));
    let (nrows, ncols) = if cells.is_empty() { (1, 1) } else { ((r1 - r0).saturating_add(1), (c1 - c0).saturating_add(1)) };
    let ncols_kept = ncols.min(MAX_COLS);
    if ncols_kept < ncols {
        warnings.push(format!("only the first {ncols_kept} columns were placed"));
    }
    let nrows_kept = nrows.min((MAX_CELLS / ncols_kept).max(1));
    if nrows_kept < nrows {
        warnings.push(format!("only the first {nrows_kept} rows were placed"));
    }
    let mut rows: Vec<Vec<String>> = vec![vec![String::new(); ncols_kept]; nrows_kept];
    for ((r, c), v) in cells {
        if let Some(cell) = rows.get_mut(r - r0).and_then(|row| row.get_mut(c - c0)) {
            *cell = v;
        }
    }
    let mut story = Story::new(StoryId(0));
    let width = 72.0 * ncols_kept as f64;
    let t = Table::from_strings(1, &rows, width, &ParaFormat::default(), &CharFormat::default());
    story.insert_table(0, t);
    Ok(Imported { story, para_styles: vec![], char_styles: vec![], warnings })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn workbook() -> Vec<u8> {
        workbook_with(
            r#"<worksheet><sheetData><row r="2"><c r="B2" t="s"><v>0</v></c><c r="C2" t="s"><v>1</v></c></row><row r="3"><c r="B3" t="s"><v>2</v></c><c r="C3"><v>3.5</v></c></row><row r="4"><c r="B4" t="inlineStr"><is><t>Cake</t></is></c><c r="C4"><v>12.0</v></c></row></sheetData></worksheet>"#,
        )
    }

    fn workbook_with(sheet: &str) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let o = zip::write::SimpleFileOptions::default();
            for (name, body) in [
                (
                    "xl/workbook.xml",
                    r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Prices" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
                ),
                (
                    "xl/_rels/workbook.xml.rels",
                    r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="x" Target="worksheets/sheet1.xml"/></Relationships>"#,
                ),
                ("xl/sharedStrings.xml", r#"<sst><si><t>Item</t></si><si><t>Price</t></si><si><r><t>Te</t></r><r><t>a</t></r></si></sst>"#),
                ("xl/worksheets/sheet1.xml", sheet),
            ] {
                z.start_file(name, o).unwrap();
                z.write_all(body.as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        buf.into_inner()
    }

    /// A `t="s"` cell materialises its own copy of the shared string, so one big string with many
    /// references multiplies: the audit's case is a 1 MB string and 3M cells, terabytes, built
    /// before the grid cap applies (#1584). Collection now carries its own byte budget.
    #[test]
    fn a_shared_string_referenced_many_times_does_not_multiply() {
        // A 64 KB shared string referenced 2,000 times would be 128 MB of cell text; the budget
        // is 64 MB, so collection stops and says so.
        let big = "y".repeat(64 * 1024);
        let cells: String = (1..=2_000).map(|r| format!(r#"<row r="{r}"><c r="A{r}" t="s"><v>0</v></c></row>"#)).collect();
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let o = zip::write::SimpleFileOptions::default();
            for (name, body) in [
                (
                    "xl/workbook.xml",
                    r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Big" sheetId="1" r:id="rId1"/></sheets></workbook>"#.to_string(),
                ),
                (
                    "xl/_rels/workbook.xml.rels",
                    r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="x" Target="worksheets/sheet1.xml"/></Relationships>"#.to_string(),
                ),
                ("xl/sharedStrings.xml", format!("<sst><si><t>{big}</t></si></sst>")),
                ("xl/worksheets/sheet1.xml", format!("<worksheet><sheetData>{cells}</sheetData></worksheet>")),
            ] {
                z.start_file(name, o).unwrap();
                z.write_all(body.as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        let imp = import(&buf.into_inner()).unwrap();
        let table = imp.story.tables.values().next().expect("a table");
        let text: usize = (0..table.nrows())
            .flat_map(|r| (0..table.ncols()).map(move |c| (r, c)))
            .filter_map(|(r, c)| table.cell(r, c).map(|cell| cell.text.text.len()))
            .sum();
        assert!(text <= 64 << 20, "the table holds at most the budget: {text} bytes");
        assert!(imp.warnings.iter().any(|w| w.contains("was not read")), "the report says the sheet was cut: {:?}", imp.warnings);

        // An ordinary workbook is unaffected and reports nothing of the sort.
        let plain = import(&workbook()).unwrap();
        assert!(!plain.warnings.iter().any(|w| w.contains("was not read")), "{:?}", plain.warnings);
    }

    #[test]
    fn first_sheet_used_range_becomes_a_table() {
        let imp = import(&workbook()).unwrap();
        let t = imp.story.tables.values().next().expect("a table");
        assert_eq!((t.nrows(), t.ncols()), (3, 2), "the used range B2:C4");
        let cell = |r, c| t.cell(r, c).unwrap().text.text.clone();
        assert_eq!([cell(0, 0), cell(0, 1), cell(1, 0), cell(1, 1), cell(2, 0), cell(2, 1)], ["Item", "Price", "Tea", "3.5", "Cake", "12"]);
        assert_eq!(cell_ref("AA10"), Some((9, 26)));
    }

    /// A cell far away from the others used to allocate the whole used range (billions of cells)
    /// before the cell cap applied; a long column name overflowed the column number.
    #[test]
    fn far_away_cell_references_are_capped() {
        let wb = workbook_with(
            r#"<worksheet><sheetData><row><c r="A1" t="inlineStr"><is><t>a</t></is></c><c r="A999999999" t="inlineStr"><is><t>z</t></is></c><c r="XFDXFDXFDXFDXFDXFD1" t="inlineStr"><is><t>far</t></is></c></row></sheetData></worksheet>"#,
        );
        let imp = import(&wb).unwrap();
        let t = imp.story.tables.values().next().expect("a table");
        assert!(t.nrows() * t.ncols() <= 100_000);
        assert!(!imp.warnings.is_empty());
        assert_eq!(cell_ref("XFDXFDXFDXFDXFDXFD1"), None);
    }

    fn sheets(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let o = zip::write::SimpleFileOptions::default();
            for (name, body) in parts {
                z.start_file(*name, o).unwrap();
                z.write_all(body.as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn records_keep_empty_cells_cached_values_and_a_named_sheet() {
        let first = workbook();
        let rows = records(&first, None).unwrap();
        assert_eq!(rows[0], vec!["Item".to_string(), "Price".to_string()]);
        assert_eq!(rows[1], vec!["Tea".to_string(), "3.5".to_string()]);
        assert_eq!(rows[2][1], "12");
        let book = sheets(&[
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
                r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Name</t></is></c><c r="B1" t="inlineStr"><is><t>Qty</t></is></c><c r="C1" t="inlineStr"><is><t>On</t></is></c><c r="D1" t="inlineStr"><is><t>When</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>Ada</t></is></c><c r="B2"><v>12.0</v></c><c r="C2" t="b"><v>1</v></c><c r="D2"><v>44927</v></c></row><row r="3"><c r="A3" t="inlineStr"><is><t>Grace</t></is></c><c r="C3"><f>1+1</f></c></row><row r="4"><c r="B4"><v>1</v></c></row></sheetData></worksheet>"#,
            ),
        ]);
        let rows = records(&book, Some("People")).unwrap();
        assert_eq!(rows[0], vec!["Name".to_string(), "Qty".to_string(), "On".to_string(), "When".to_string()]);
        assert_eq!(rows[1], vec!["Ada".to_string(), "12".to_string(), "TRUE".to_string(), "44927".to_string()]);
        // Formula with no cached value is empty. The short row is padded. The all-empty gap is skipped
        // only when every cell is empty; row 4 has a Qty, so it stays, and Name is empty.
        assert_eq!(rows[2], vec!["Grace".to_string(), String::new(), String::new(), String::new()]);
        assert_eq!(rows[3][0], "");
        assert_eq!(rows[3][1], "1");
        assert!(records(&book, Some("Missing")).is_err());
    }

    #[test]
    fn phonetic_guide_text_is_not_cell_text() {
        let book = sheets(&[
            (
                "xl/workbook.xml",
                r#"<workbook xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="S" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
            ),
            ("xl/_rels/workbook.xml.rels", r#"<Relationships><Relationship Id="rId1" Type="x" Target="worksheets/sheet1.xml"/></Relationships>"#),
            (
                "xl/sharedStrings.xml",
                r#"<sst><si><t>日本</t><rPh sb="0" eb="2"><t>ニホン</t></rPh><phoneticPr fontId="1"/></si><si><r><t>東</t></r><r><t>京</t></r><rPh sb="0" eb="2"><t>トウキョウ</t></rPh></si></sst>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1" t="inlineStr"><is><t>大阪</t><rPh sb="0" eb="2"><t>オオサカ</t></rPh><phoneticPr/></is></c></row></sheetData></worksheet>"#,
            ),
        ]);
        let want = ["日本", "東京", "大阪"].map(String::from).to_vec();
        assert_eq!(records(&book, None).unwrap(), vec![want.clone()]);
        let imp = import(&book).unwrap();
        let t = imp.story.tables.values().next().expect("a table");
        let got: Vec<String> = (0..3).map(|c| t.cell(0, c).unwrap().text.text.clone()).collect();
        assert_eq!(got, want);
    }

    fn one_value(cell: &str) -> Vec<u8> {
        workbook_with(&format!(
            r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Value</t></is></c></row><row r="2">{cell}</row></sheetData></worksheet>"#
        ))
    }

    #[test]
    fn numeric_values_keep_their_precision_in_tables_and_records() {
        for text in ["1e-12", "-0.0000000000123", "1.23456789012345", "1e300", "12.0", "-2.5e-300"] {
            let book = one_value(&format!(r#"<c r="A2"><v>{text}</v></c>"#));
            let placed = crate::import("values.xlsx", &book).unwrap();
            let table = placed.story.tables.values().next().unwrap();
            let rows = crate::xlsx_records(&book, None).unwrap();
            for got in [&table.cell(1, 0).unwrap().text.text, &rows[1][0]] {
                assert_eq!(got.parse::<f64>().unwrap(), text.parse::<f64>().unwrap(), "cell {text} became {got}");
            }
        }
    }

    #[test]
    fn numeric_values_show_as_excel_general_format() {
        for (text, shown) in [
            ("0.30000000000000004", "0.3"),
            ("2.0000000000000004", "2"),
            ("12.0", "12"),
            ("-0.5", "-0.5"),
            ("0", "0"),
            ("123456789012345", "123456789012345"),
            ("1e15", "1E+15"),
            ("1e300", "1E+300"),
            ("1e-12", "1E-12"),
            ("-1.5e-10", "-1.5E-10"),
            ("inf", "inf"),
        ] {
            let book = one_value(&format!(r#"<c r="A2"><v>{text}</v></c>"#));
            let rows = crate::xlsx_records(&book, None).unwrap();
            assert_eq!(rows[1][0], shown, "cell {text}");
        }
    }

    #[test]
    fn cached_string_values_are_not_reformatted_as_numbers() {
        for text in ["00123", "1e3", "12.0", " 42 "] {
            let book = one_value(&format!(r#"<c r="A2" t="str"><f>"{text}"</f><v>{text}</v></c>"#));
            let placed = crate::import("values.xlsx", &book).unwrap();
            let table = placed.story.tables.values().next().unwrap();
            let rows = crate::xlsx_records(&book, None).unwrap();
            assert_eq!(table.cell(1, 0).unwrap().text.text, text);
            assert_eq!(rows[1][0], text);
        }
    }
}
