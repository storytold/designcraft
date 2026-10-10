//! Excel workbooks (`.xlsx`) → a story holding one table: the first worksheet's used range, cell
//! values as shown text (shared and inline strings, numbers, booleans). Data merge uses
//! [`records`], which keeps empty cells and can name a sheet.

use std::collections::{BTreeMap, HashMap};

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
    let sheet_xml = parse(&part(&mut zip, &sheet_part)?.ok_or_else(|| ImportError::Corrupt(format!("no {sheet_part} (not an Excel workbook)")))?)?;
    let shared: Vec<String> = part(&mut zip, "xl/sharedStrings.xml")?
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
    assemble(cells)
}

/// Cached values, computed formulas, and date text for data merge. [`records`] stays cached-only.
pub fn merge_records(bytes: &[u8], sheet: Option<&str>) -> Result<(Vec<Vec<String>>, Vec<String>), ImportError> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| ImportError::Corrupt(e.to_string()))?;
    let sheet_part = sheet_target(&mut zip, sheet)?;
    let sheet_xml = parse(&part(&mut zip, &sheet_part)?.ok_or_else(|| ImportError::Corrupt(format!("no {sheet_part} (not an Excel workbook)")))?)?;
    let shared: Vec<String> = part(&mut zip, "xl/sharedStrings.xml")?
        .map(|x| parse(&x))
        .transpose()?
        .map(|sst| sst.els().filter(|e| e.name == "si").map(El::text).collect())
        .unwrap_or_default();
    let styles = part(&mut zip, "xl/styles.xml")?.and_then(|xml| parse(&xml).ok()).map(|root| style_table(&root)).unwrap_or_default();
    let mut stored: HashMap<(usize, usize), Stored> = HashMap::new();
    if let Some(data) = sheet_xml.child("sheetData") {
        for (ri, row) in data.els().filter(|e| e.name == "row").enumerate() {
            for (ci, c) in row.els().filter(|e| e.name == "c").enumerate() {
                let (r, col) = c.attr("r").and_then(cell_ref).unwrap_or((ri, ci));
                stored.insert((r, col), stored_cell(c, &shared));
            }
        }
    }
    if stored.is_empty() {
        return Err(ImportError::Corrupt("the worksheet is empty".into()));
    }
    let mut warnings = Vec::new();
    let mut cells = Vec::with_capacity(stored.len());
    let mut keys: Vec<(usize, usize)> = stored.keys().copied().collect();
    keys.sort_unstable();
    for key in keys {
        let (text, notes) = display_cell(&stored, key, &styles);
        warnings.extend(notes);
        cells.push((key.0, key.1, text));
    }
    Ok((assemble(cells)?, warnings))
}

struct Stored {
    text: String,
    number: Option<f64>,
    formula: Option<String>,
    style: Option<usize>,
    plain: bool,
}

fn stored_cell(c: &El, shared: &[String]) -> Stored {
    let kind = c.attr("t");
    let plain = !matches!(kind, Some("s" | "inlineStr" | "b" | "str" | "e"));
    let number = if plain { c.child("v").map(El::text).and_then(|s| s.trim().parse::<f64>().ok()).filter(|n| n.is_finite()) } else { None };
    let has_display = c.child("v").is_some() || matches!(kind, Some("inlineStr" | "s" | "b"));
    Stored {
        text: if has_display { value(c, shared) } else { String::new() },
        number,
        formula: c.child("f").map(El::text).filter(|f| !f.trim().is_empty()),
        style: c.attr("s").and_then(|s| s.parse().ok()),
        plain,
    }
}

fn display_cell(sheet: &HashMap<(usize, usize), Stored>, key: (usize, usize), styles: &StyleTable) -> (String, Vec<String>) {
    let Some(cell) = sheet.get(&key) else { return (String::new(), Vec::new()) };
    let addr = cell_addr(key.0, key.1);
    if cell.number.is_some() || (cell.formula.is_some() && !cell.text.is_empty()) || cell.formula.is_none() {
        return apply_date(cell.number, cell.text.clone(), cell.plain, cell.style, styles, &addr);
    }
    let mut stack = Vec::new();
    match eval_formula(cell.formula.as_deref().unwrap_or(""), sheet, &mut stack, 0) {
        Ok(n) => apply_date(Some(n), num_text(n), true, cell.style, styles, &addr),
        Err(()) => (String::new(), vec![format!("Cell {addr}: the formula could not be computed.")]),
    }
}

fn apply_date(number: Option<f64>, text: String, plain: bool, style: Option<usize>, styles: &StyleTable, addr: &str) -> (String, Vec<String>) {
    let Some(n) = number.filter(|_| plain) else { return (text, Vec::new()) };
    let Some(fmt) = style.and_then(|s| styles.date_format(s)) else { return (text, Vec::new()) };
    match format_serial(&fmt, n) {
        Ok(rendered) => (rendered, Vec::new()),
        Err(()) => (text, vec![format!("Cell {addr}: the date format could not be read.")]),
    }
}

fn num_text(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 { format!("{}", f as i64) } else { format!("{}", (f * 1e10).round() / 1e10) }
}

fn cell_addr(row: usize, col: usize) -> String {
    let mut n = col.saturating_add(1);
    let mut name = String::new();
    while n > 0 {
        n -= 1;
        let ch = char::from(b'A' + (n % 26) as u8);
        name.insert(0, ch);
        n /= 26;
    }
    format!("{name}{}", row.saturating_add(1))
}

#[derive(Clone, Debug, Default)]
struct StyleTable {
    custom: HashMap<u32, String>,
    xfs: Vec<u32>,
}

impl StyleTable {
    fn date_format(&self, index: usize) -> Option<String> {
        let id = *self.xfs.get(index)?;
        if let Some(code) = self.custom.get(&id) {
            return is_date_format(code).then(|| code.clone());
        }
        builtin_date(id).map(str::to_string)
    }
}

fn style_table(root: &El) -> StyleTable {
    let mut table = StyleTable::default();
    if let Some(fmts) = root.child("numFmts") {
        for fmt in fmts.els().filter(|e| e.name == "numFmt") {
            if let (Some(id), Some(code)) = (fmt.attr("numFmtId").and_then(|s| s.parse().ok()), fmt.attr("formatCode")) {
                table.custom.insert(id, code.to_string());
            }
        }
    }
    if let Some(xfs) = root.child("cellXfs") {
        for xf in xfs.els().filter(|e| e.name == "xf") {
            table.xfs.push(xf.attr("numFmtId").and_then(|s| s.parse().ok()).unwrap_or(0));
        }
    }
    table
}

fn builtin_date(id: u32) -> Option<&'static str> {
    Some(match id {
        14 => "mm-dd-yy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yy h:mm",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mmss.0",
        _ => return None,
    })
}

fn is_date_format(code: &str) -> bool {
    let bytes = code.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += 1;
                }
                i = i.saturating_add(1);
            }
            b'\\' => i = i.saturating_add(2),
            b'[' => {
                while i < bytes.len() && bytes[i] != b']' {
                    i += 1;
                }
                i = i.saturating_add(1);
            }
            c if c.is_ascii_alphabetic() => {
                let lower = c.to_ascii_lowercase();
                if matches!(lower, b'y' | b'm' | b'd' | b'h' | b's') {
                    return true;
                }
                while i < bytes.len() && bytes[i].to_ascii_lowercase() == lower {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    false
}

fn format_serial(fmt: &str, serial: f64) -> Result<String, ()> {
    if !serial.is_finite() {
        return Err(());
    }
    let tokens = tokenize_format(fmt)?;
    let tokens = resolve_months(tokens);
    let (year, month, day, hour, minute, second, frac) = excel_parts(serial).ok_or(())?;
    let am = tokens.iter().any(|t| matches!(t, Tok::AmPm));
    let mut out = String::new();
    for tok in &tokens {
        match tok {
            Tok::Year(n) => {
                let y = if *n >= 3 { year } else { year.rem_euclid(100) };
                let width = (*n).max(if *n >= 3 { 4 } else { 2 });
                out.push_str(&pad(y as u32, width));
            }
            Tok::Month(n) => out.push_str(&month_text(month, *n)),
            Tok::Day(n) => out.push_str(&day_text(day, *n, serial)?),
            Tok::Hour(n) => {
                let h = if am { hour12(hour) } else { hour };
                out.push_str(&pad(h, if *n > 1 { 2 } else { 1 }));
            }
            Tok::Minute(n) => out.push_str(&pad(minute, if *n > 1 { 2 } else { 1 })),
            Tok::Second(n) => out.push_str(&pad(second, if *n > 1 { 2 } else { 1 })),
            Tok::Frac(n) => {
                let scale = 10u32.saturating_pow((*n).min(6) as u32);
                let digits = ((frac * f64::from(scale)).round() as u32) % scale;
                out.push('.');
                out.push_str(&pad(digits, *n));
            }
            Tok::AmPm => out.push_str(if hour < 12 { "AM" } else { "PM" }),
            Tok::ElapsedHour => out.push_str(&format!("{}", (serial.abs() * 24.0).floor() as i64)),
            Tok::ElapsedMinute => out.push_str(&format!("{}", (serial.abs() * 24.0 * 60.0).floor() as i64)),
            Tok::ElapsedSecond => out.push_str(&format!("{}", (serial.abs() * 86400.0).floor() as i64)),
            Tok::Lit(s) => out.push_str(s),
            Tok::PendingM(_) => return Err(()),
        }
    }
    Ok(out)
}

fn pad(n: u32, width: usize) -> String {
    let s = n.to_string();
    if s.len() >= width { s } else { format!("{}{s}", "0".repeat(width - s.len())) }
}

fn month_text(month: u32, n: usize) -> String {
    const SHORT: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    const LONG: [&str; 12] =
        ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    let idx = month.saturating_sub(1) as usize;
    match n {
        0 | 1 => month.to_string(),
        2 => pad(month, 2),
        3 => SHORT.get(idx).copied().unwrap_or("").to_string(),
        4 => LONG.get(idx).copied().unwrap_or("").to_string(),
        _ => SHORT.get(idx).and_then(|s| s.chars().next()).map(|c| c.to_string()).unwrap_or_default(),
    }
}

fn day_text(day: u32, n: usize, serial: f64) -> Result<String, ()> {
    const SHORT: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const LONG: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
    match n {
        0 | 1 => Ok(day.to_string()),
        2 => Ok(pad(day, 2)),
        _ => {
            let wd = weekday(serial)?;
            Ok(if n == 3 { SHORT.get(wd).copied().unwrap_or("").to_string() } else { LONG.get(wd).copied().unwrap_or("").to_string() })
        }
    }
}

fn hour12(hour: u32) -> u32 {
    let h = hour % 12;
    if h == 0 { 12 } else { h }
}

fn weekday(serial: f64) -> Result<usize, ()> {
    if serial.round() as i64 == 60 {
        return Ok(3);
    }
    let unix = excel_unix_days(serial).ok_or(())?;
    Ok((unix + 4).rem_euclid(7) as usize)
}

/// Excel 1900 date system. Serial 1 is 1900-01-01. Serial 60 is the fake 1900-02-29.
fn excel_parts(serial: f64) -> Option<(i32, u32, u32, u32, u32, u32, f64)> {
    let whole = serial.trunc() as i64;
    let (year, month, day) = if whole == 60 {
        (1900, 2, 29)
    } else {
        let unix = excel_unix_days(serial)?;
        civil_from_days(unix)?
    };
    let frac = serial.fract().abs();
    let secs = (frac * 86400.0).round() as i64;
    let secs = if secs >= 86400 { 0 } else { secs };
    let hour = (secs / 3600) as u32;
    let minute = ((secs % 3600) / 60) as u32;
    let second = (secs % 60) as u32;
    let sub = (frac * 86400.0 - secs as f64).abs();
    Some((year, month, day, hour, minute, second, sub))
}

fn excel_unix_days(serial: f64) -> Option<i64> {
    let whole = serial.trunc() as i64;
    if !(0..=2_958_465).contains(&whole) {
        return None;
    }
    let shifted = if whole > 60 { whole - 1 } else { whole };
    Some(shifted - 25568)
}

fn civil_from_days(z: i64) -> Option<(i32, u32, u32)> {
    let z = z.checked_add(719468)?;
    let era = if z >= 0 { z } else { z.checked_sub(146096)? } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    Some((y as i32, m as u32, d as u32))
}

#[derive(Clone, Debug)]
enum Tok {
    Year(usize),
    Month(usize),
    Day(usize),
    Hour(usize),
    Minute(usize),
    Second(usize),
    Frac(usize),
    AmPm,
    ElapsedHour,
    ElapsedMinute,
    ElapsedSecond,
    Lit(String),
    PendingM(usize),
}

fn tokenize_format(fmt: &str) -> Result<Vec<Tok>, ()> {
    let bytes = fmt.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'"' {
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            out.push(Tok::Lit(String::from_utf8_lossy(&bytes[start..i]).into_owned()));
            if i < bytes.len() {
                i += 1;
            }
            continue;
        }
        if c == b'\\' {
            i += 1;
            if i >= bytes.len() {
                return Err(());
            }
            out.push(Tok::Lit(char::from(bytes[i]).to_string()));
            i += 1;
            continue;
        }
        if c == b'[' {
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != b']' {
                i += 1;
            }
            let inner = String::from_utf8_lossy(&bytes[start..i]).to_ascii_lowercase();
            if i < bytes.len() {
                i += 1;
            }
            match inner.as_str() {
                "h" | "hh" => out.push(Tok::ElapsedHour),
                "m" | "mm" => out.push(Tok::ElapsedMinute),
                "s" | "ss" => out.push(Tok::ElapsedSecond),
                _ => {}
            }
            continue;
        }
        let rest = fmt.get(i..).unwrap_or("").to_ascii_lowercase();
        if rest.starts_with("am/pm") {
            out.push(Tok::AmPm);
            i += 5;
            continue;
        }
        if rest.starts_with("a/p") {
            out.push(Tok::AmPm);
            i += 3;
            continue;
        }
        if c == b'.' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] == b'0' {
                j += 1;
            }
            if j > i + 1 {
                out.push(Tok::Frac(j - i - 1));
                i = j;
                continue;
            }
        }
        if c.is_ascii_alphabetic() {
            let lower = c.to_ascii_lowercase();
            let start = i;
            while i < bytes.len() && bytes[i].to_ascii_lowercase() == lower {
                i += 1;
            }
            let n = i - start;
            match lower {
                b'y' => out.push(Tok::Year(n)),
                b'd' => out.push(Tok::Day(n)),
                b'h' => out.push(Tok::Hour(n)),
                b's' => out.push(Tok::Second(n)),
                b'm' => out.push(Tok::PendingM(n)),
                _ => return Err(()),
            }
            continue;
        }
        if matches!(c, b'#' | b'?') {
            return Err(());
        }
        if c == b'0' {
            return Err(());
        }
        out.push(Tok::Lit(char::from(c).to_string()));
        i += 1;
    }
    Ok(out)
}

fn resolve_months(tokens: Vec<Tok>) -> Vec<Tok> {
    let mut out = tokens;
    for i in 0..out.len() {
        if !matches!(out[i], Tok::PendingM(_)) {
            continue;
        }
        let prev = (0..i).rev().find_map(|j| sigil(&out[j]));
        let next = (i + 1..out.len()).find_map(|j| sigil(&out[j]));
        let minute = matches!(prev, Some('h' | 's')) || matches!(next, Some('h' | 's'));
        let n = match out[i] {
            Tok::PendingM(n) => n,
            _ => 1,
        };
        out[i] = if minute { Tok::Minute(n) } else { Tok::Month(n) };
    }
    out
}

fn sigil(tok: &Tok) -> Option<char> {
    match tok {
        Tok::Year(_) | Tok::Day(_) => Some('d'),
        Tok::Hour(_) | Tok::ElapsedHour => Some('h'),
        Tok::Second(_) | Tok::ElapsedSecond | Tok::Frac(_) => Some('s'),
        Tok::Minute(_) | Tok::ElapsedMinute | Tok::PendingM(_) => Some('m'),
        Tok::Month(_) => Some('d'),
        _ => None,
    }
}

fn eval_formula(formula: &str, sheet: &HashMap<(usize, usize), Stored>, stack: &mut Vec<(usize, usize)>, depth: u32) -> Result<f64, ()> {
    if depth > 32 {
        return Err(());
    }
    let text = formula.trim().trim_start_matches('=').trim();
    let mut p = Form { s: text.as_bytes(), i: 0 };
    let v = p.expr(sheet, stack, depth)?;
    p.skip();
    if p.i != p.s.len() || !v.is_finite() { Err(()) } else { Ok(v) }
}

struct Form<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Form<'a> {
    fn skip(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn eat(&mut self, c: u8) -> bool {
        self.skip();
        if self.i < self.s.len() && self.s[self.i] == c {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn expr(&mut self, sheet: &HashMap<(usize, usize), Stored>, stack: &mut Vec<(usize, usize)>, depth: u32) -> Result<f64, ()> {
        if depth > 64 {
            return Err(());
        }
        let mut v = self.term(sheet, stack, depth)?;
        loop {
            self.skip();
            if self.eat(b'+') {
                v += self.term(sheet, stack, depth)?;
            } else if self.i < self.s.len() && self.s[self.i] == b'-' {
                self.i += 1;
                v -= self.term(sheet, stack, depth)?;
            } else {
                break;
            }
        }
        Ok(v)
    }
    fn term(&mut self, sheet: &HashMap<(usize, usize), Stored>, stack: &mut Vec<(usize, usize)>, depth: u32) -> Result<f64, ()> {
        let mut v = self.factor(sheet, stack, depth)?;
        loop {
            self.skip();
            if self.eat(b'*') {
                v *= self.factor(sheet, stack, depth)?;
            } else if self.eat(b'/') {
                let d = self.factor(sheet, stack, depth)?;
                if d == 0.0 {
                    return Err(());
                }
                v /= d;
            } else {
                break;
            }
        }
        Ok(v)
    }
    fn factor(&mut self, sheet: &HashMap<(usize, usize), Stored>, stack: &mut Vec<(usize, usize)>, depth: u32) -> Result<f64, ()> {
        self.skip();
        if self.eat(b'+') {
            return self.factor(sheet, stack, depth.saturating_add(1));
        }
        if self.eat(b'-') {
            return Ok(-self.factor(sheet, stack, depth.saturating_add(1))?);
        }
        if self.eat(b'(') {
            let v = self.expr(sheet, stack, depth.saturating_add(1))?;
            if !self.eat(b')') {
                return Err(());
            }
            return Ok(v);
        }
        if self.starts_sum() {
            self.i += 3;
            if !self.eat(b'(') {
                return Err(());
            }
            let ((r0, c0), (r1, c1)) = self.range()?;
            if !self.eat(b')') {
                return Err(());
            }
            let rows = r1.saturating_sub(r0).saturating_add(1);
            let cols = c1.saturating_sub(c0).saturating_add(1);
            if rows.saturating_mul(cols) > 100_000 {
                return Err(());
            }
            let mut sum = 0.0;
            for r in r0..=r1 {
                for c in c0..=c1 {
                    if let Ok(n) = cell_number(sheet, (r, c), stack, depth.saturating_add(1)) {
                        sum += n;
                    }
                }
            }
            return Ok(sum);
        }
        if self.i < self.s.len() && (self.s[self.i].is_ascii_digit() || self.s[self.i] == b'.') {
            return self.number();
        }
        if self.i < self.s.len() && (self.s[self.i].is_ascii_alphabetic() || self.s[self.i] == b'$') {
            let (r, c) = self.cell()?;
            return cell_number(sheet, (r, c), stack, depth.saturating_add(1));
        }
        Err(())
    }
    fn starts_sum(&self) -> bool {
        let rest = self.s.get(self.i..).unwrap_or(&[]);
        rest.len() >= 4 && rest[..3].eq_ignore_ascii_case(b"SUM") && (rest[3] == b'(' || rest[3].is_ascii_whitespace())
    }
    fn number(&mut self) -> Result<f64, ()> {
        let start = self.i;
        while self.i < self.s.len() && (self.s[self.i].is_ascii_digit() || self.s[self.i] == b'.') {
            self.i += 1;
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).map_err(|_| ())?;
        text.parse::<f64>().ok().filter(|n| n.is_finite()).ok_or(())
    }
    fn cell(&mut self) -> Result<(usize, usize), ()> {
        self.skip();
        let _ = self.eat(b'$');
        let start = self.i;
        while self.i < self.s.len() && self.s[self.i].is_ascii_alphabetic() {
            self.i += 1;
        }
        if self.i == start {
            return Err(());
        }
        let letters = std::str::from_utf8(&self.s[start..self.i]).map_err(|_| ())?;
        let _ = self.eat(b'$');
        let rs = self.i;
        while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
            self.i += 1;
        }
        if self.i == rs {
            return Err(());
        }
        let digits = std::str::from_utf8(&self.s[rs..self.i]).map_err(|_| ())?;
        cell_ref(&format!("{letters}{digits}")).ok_or(())
    }
    fn range(&mut self) -> Result<((usize, usize), (usize, usize)), ()> {
        let a = self.cell()?;
        self.skip();
        if self.eat(b':') {
            let b = self.cell()?;
            Ok(((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1))))
        } else {
            Ok((a, a))
        }
    }
}

fn cell_number(sheet: &HashMap<(usize, usize), Stored>, key: (usize, usize), stack: &mut Vec<(usize, usize)>, depth: u32) -> Result<f64, ()> {
    if depth > 32 || stack.contains(&key) {
        return Err(());
    }
    let Some(cell) = sheet.get(&key) else { return Err(()) };
    if let Some(n) = cell.number {
        return Ok(n);
    }
    if cell.formula.is_some() && cell.text.is_empty() {
        stack.push(key);
        let result = eval_formula(cell.formula.as_deref().unwrap_or(""), sheet, stack, depth);
        stack.pop();
        return result;
    }
    cell.text.trim().parse::<f64>().ok().filter(|n| n.is_finite()).ok_or(())
}

/// One worksheet as a grid. The first used row is the header. Empty cells inside that width are
/// kept. Cells past the header are kept so the caller can warn.
pub(crate) fn assemble(cells: Vec<(usize, usize, String)>) -> Result<Vec<Vec<String>>, ImportError> {
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

    #[test]
    fn merge_records_prefer_a_cached_value_compute_a_formula_and_format_dates() {
        let sheet = r#"<worksheet><sheetData>
<row r="1"><c r="A1" t="inlineStr"><is><t>Name</t></is></c><c r="B1" t="inlineStr"><is><t>Qty</t></is></c><c r="C1" t="inlineStr"><is><t>When</t></is></c><c r="D1" t="inlineStr"><is><t>Other</t></is></c><c r="E1" t="inlineStr"><is><t>Built</t></is></c></row>
<row r="2"><c r="A2" t="inlineStr"><is><t>Ada</t></is></c><c r="B2"><v>9</v><f>1+1</f></c><c r="C2" s="1"><v>1</v></c><c r="D2"><v>12</v></c><c r="E2" s="3"><v>1</v></c></row>
<row r="3"><c r="A3" t="inlineStr"><is><t>Bea</t></is></c><c r="B3"><f>1+1</f></c><c r="C3" s="1"><v>60</v></c><c r="D3" s="1"><v>61</v></c></row>
<row r="4"><c r="A4" t="inlineStr"><is><t>Cy</t></is></c><c r="B4"><f>SUM(B2:B3)</f></c><c r="C4" s="2"><v>1</v></c><c r="D4"><f>NOW()</f></c></row>
<row r="5"><c r="A5" t="inlineStr"><is><t>Dee</t></is></c><c r="B5"><f>A2&amp;&quot;x&quot;</f></c></row>
</sheetData></worksheet>"#;
        let book = sheets(&[
            (
                "xl/workbook.xml",
                r#"<workbook xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="People" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
            ),
            ("xl/_rels/workbook.xml.rels", r#"<Relationships><Relationship Id="rId1" Type="x" Target="worksheets/sheet1.xml"/></Relationships>"#),
            ("xl/worksheets/sheet1.xml", sheet),
            (
                "xl/styles.xml",
                r#"<styleSheet><numFmts><numFmt numFmtId="164" formatCode="yyyy-mm-dd"/><numFmt numFmtId="165" formatCode="yyyy-qq"/></numFmts><cellXfs><xf numFmtId="0"/><xf numFmtId="164"/><xf numFmtId="165"/><xf numFmtId="14"/></cellXfs></styleSheet>"#,
            ),
        ]);
        let cached = records(&book, None).unwrap();
        assert_eq!(cached[1], vec!["Ada".to_string(), "9".to_string(), "1".to_string(), "12".to_string(), "1".to_string()]);
        assert_eq!(cached[2][1], "", "records() leaves an uncached formula empty");
        assert_eq!(cached[2][2], "60");
        let (rows, warnings) = merge_records(&book, None).unwrap();
        assert_eq!(rows[1], vec!["Ada".to_string(), "9".to_string(), "1900-01-01".to_string(), "12".to_string(), "01-01-00".to_string()]);
        assert_eq!(rows[2], vec!["Bea".to_string(), "2".to_string(), "1900-02-29".to_string(), "1900-03-01".to_string(), String::new()]);
        assert_eq!(rows[3][1], "11", "SUM of the cached 9 and the computed 2");
        assert_eq!(rows[3][2], "1", "an unreadable date format keeps the serial");
        assert_eq!(rows[3][3], "");
        assert_eq!(rows[4][1], "");
        let notes = warnings.join("\n");
        assert!(notes.contains("Cell C4: the date format could not be read."), "{notes}");
        assert!(notes.contains("Cell D4: the formula could not be computed."), "{notes}");
        assert!(notes.contains("Cell B5: the formula could not be computed."), "{notes}");
        let imp = import(&book).unwrap();
        let t = imp.story.tables.values().next().expect("a table");
        let cell = |r, c| t.cell(r, c).unwrap().text.text.clone();
        assert_eq!(cell(1, 1), "9");
        assert_eq!(cell(1, 2), "1");
        assert_ne!(cell(2, 1), "2");
        let flat: Vec<String> = (0..t.nrows()).flat_map(|r| (0..t.ncols()).map(move |c| cell(r, c))).collect();
        assert!(flat.iter().all(|c| !c.contains("1900")), "{flat:?}");
    }
}
