//! Excel workbooks (`.xlsx`) → a story holding one table: the first worksheet's used range, cell
//! values as shown text (shared and inline strings, numbers, booleans). Data merge uses
//! [`records`], which keeps empty cells and can name a sheet.

use std::collections::BTreeMap;

use designcraft_doc::{CharFormat, ParaFormat, Story, StoryId, Table};

use crate::docx::{El, parse, part};
use crate::{ImportError, Imported};

/// "B12" → (row 11, column 1).
fn cell_ref(r: &str) -> Option<(usize, usize)> {
    let letters: String = r.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let row: usize = r[letters.len()..].parse().ok()?;
    let col = letters.chars().try_fold(0usize, |acc, c| {
        let digit = (c.to_ascii_uppercase() as u8).checked_sub(b'A')? as usize + 1;
        acc.checked_mul(26)?.checked_add(digit)
    })?;
    Some((row.checked_sub(1)?, col.checked_sub(1)?))
}

/// The worksheet part of the first sheet (workbook order), via the workbook relationships.
fn first_sheet(zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>) -> Option<String> {
    let wb = parse(&part(zip, "xl/workbook.xml")?).ok()?;
    let rid = wb.child("sheets")?.els().find(|e| e.name == "sheet")?.attr("id")?.to_string();
    let rels = parse(&part(zip, "xl/_rels/workbook.xml.rels")?).ok()?;
    let target = rels.els().find(|r| r.attr("Id") == Some(rid.as_str()))?.attr("Target")?.trim_start_matches('/').to_string();
    Some(if target.starts_with("xl/") { target } else { format!("xl/{target}") })
}

fn value(c: &El, shared: &[String]) -> String {
    let v = || c.child("v").map(El::text).unwrap_or_default();
    match c.attr("t") {
        Some("s") => v().trim().parse::<usize>().ok().and_then(|i| shared.get(i).cloned()).unwrap_or_default(),
        Some("inlineStr") => c.child("is").map(El::text).unwrap_or_default(),
        Some("b") => {
            if v().trim() == "1" {
                "TRUE".into()
            } else {
                "FALSE".into()
            }
        }
        _ => {
            let raw = v();
            // Whole numbers without the trailing ".0" Excel never shows.
            match raw.trim().parse::<f64>() {
                Ok(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", f as i64),
                Ok(f) => format!("{}", (f * 1e10).round() / 1e10),
                Err(_) => raw,
            }
        }
    }
}

/// One worksheet as a grid of cached cell text, empty cells kept, for data merge.
/// The first used row is the header. Later rows that are entirely empty are left out.
/// Cells past the header are kept so the caller can warn. Over 200 columns or 100,000
/// data rows is an error and the grid is not built.
pub fn records(bytes: &[u8], sheet: Option<&str>) -> Result<Vec<Vec<String>>, ImportError> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| ImportError::Corrupt(e.to_string()))?;
    let sheet_part = sheet_target(&mut zip, sheet)?;
    let sheet_xml = parse(&part(&mut zip, &sheet_part).ok_or_else(|| ImportError::Corrupt(format!("no {sheet_part} (not an Excel workbook)")))?)?;
    let shared: Vec<String> = part(&mut zip, "xl/sharedStrings.xml")
        .map(|x| parse(&x))
        .transpose()?
        .map(|sst| sst.els().filter(|e| e.name == "si").map(El::text).collect())
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
    let mut min_c = usize::MAX;
    let mut max_c = 0usize;
    let mut header_used = false;
    for (r, c, _) in &cells {
        if *r == header_row {
            header_used = true;
            min_c = min_c.min(*c);
            max_c = max_c.max(*c);
        }
    }
    if !header_used || min_c == usize::MAX {
        return Err(ImportError::Corrupt("the worksheet has no header".into()));
    }
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
        let mut row = vec![String::new(); width];
        let mut extra: Vec<String> = Vec::new();
        for (c, v) in cols {
            if *c < min_c {
                // Outside the header span. Keep one non-empty value so the caller can warn.
                if !v.is_empty() {
                    extra.push(v.clone());
                }
                continue;
            }
            let at = c - min_c;
            if at < width {
                if let Some(cell) = row.get_mut(at) {
                    *cell = v.clone();
                }
            } else if !v.is_empty() {
                extra.push(v.clone());
            }
        }
        if *r != header_row && row.iter().all(|c| c.is_empty()) && extra.is_empty() {
            continue;
        }
        row.extend(extra);
        out.push(row);
    }
    Ok(out)
}

/// Workbook order, or the sheet named `wanted`.
fn sheet_target(zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, wanted: Option<&str>) -> Result<String, ImportError> {
    let wb = parse(&part(zip, "xl/workbook.xml").ok_or_else(|| ImportError::Corrupt("no xl/workbook.xml (not an Excel workbook)".into()))?)?;
    let sheets: Vec<(String, String)> = wb
        .child("sheets")
        .map(|s| s.els().filter(|e| e.name == "sheet").filter_map(|e| Some((e.attr("name")?.to_string(), e.attr("id")?.to_string()))).collect())
        .unwrap_or_default();
    let (name, rid) = match wanted {
        None => sheets.first().cloned().ok_or_else(|| ImportError::Corrupt("the workbook has no sheets".into()))?,
        Some(w) => sheets.into_iter().find(|(n, _)| n == w).ok_or_else(|| ImportError::Corrupt(format!("no worksheet \"{w}\"")))?,
    };
    let rels = parse(&part(zip, "xl/_rels/workbook.xml.rels").ok_or_else(|| ImportError::Corrupt("no workbook relationships".into()))?)?;
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
    let sheet_part = first_sheet(&mut zip).unwrap_or_else(|| "xl/worksheets/sheet1.xml".into());
    let sheet = parse(&part(&mut zip, &sheet_part).ok_or_else(|| ImportError::Corrupt(format!("no {sheet_part} (not an Excel workbook)")))?)?;
    let shared: Vec<String> = part(&mut zip, "xl/sharedStrings.xml")
        .map(|x| parse(&x))
        .transpose()?
        .map(|sst| sst.els().filter(|e| e.name == "si").map(El::text).collect())
        .unwrap_or_default();
    let mut cells: Vec<((usize, usize), String)> = Vec::new();
    if let Some(data) = sheet.child("sheetData") {
        for (ri, row) in data.els().filter(|e| e.name == "row").enumerate() {
            for (ci, c) in row.els().filter(|e| e.name == "c").enumerate() {
                let at = c.attr("r").and_then(cell_ref).unwrap_or((ri, ci));
                let v = value(c, &shared);
                if !v.is_empty() {
                    cells.push((at, v));
                }
            }
        }
    }
    let mut warnings = Vec::new();
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
}
