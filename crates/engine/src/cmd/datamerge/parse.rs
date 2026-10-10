//! Pure data-merge parsing: text tables, record selection, image paths, and tile slots.
//!
//! The command layer loads bytes and copies documents. These functions only look at strings,
//! rows, and rectangles, and they refuse a table that is over the row or column cap before
//! that table is returned.

use std::path::{Path, PathBuf};

use designcraft_doc::{DataField, DataFieldKind, Delimiter, FilterRule, Fingerprint, SortKey, SourceFilter, SourceStatus};
use designcraft_geom::Rect;
use designcraft_textimport::{DATA_MERGE_MAX_COLS, DATA_MERGE_MAX_ROWS};
use serde_json::Value;

/// A parsed table. `rows` does not include the header. Every row matches `fields` in length.
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub fields: Vec<DataField>,
    pub rows: Vec<Vec<String>>,
    pub warnings: Vec<String>,
    pub delimiter: Delimiter,
}

#[derive(Clone, Debug)]
struct RawRow {
    /// 1-based line where the record starts (the header is line 1 when it is the first record).
    line: usize,
    cells: Vec<String>,
}

/// Where one merged record sits. `dx` and `dy` are the translation from the original block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileHit {
    pub index: usize,
    pub page: usize,
    pub col: i32,
    pub row: i32,
    pub dx: f64,
    pub dy: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TilePlan {
    pub hits: Vec<TileHit>,
    pub pages: usize,
    /// A second copy did not fit, so every record has its own page.
    pub one_per_page: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct TileInput {
    pub block: Rect,
    pub page_width: f64,
    pub page_height: f64,
    /// Top, right, bottom, left.
    pub insets: [f64; 4],
    pub column_spacing: f64,
    pub row_spacing: f64,
    pub rows_first: bool,
    pub count: usize,
}

/// Comma-separated records, for the existing parser test. Errors become an empty table.
#[cfg(test)]
pub fn parse_csv(s: &str) -> Vec<Vec<String>> {
    parse_delimited(s, ',').map(|rows| rows.into_iter().map(|r| r.cells).collect()).unwrap_or_default()
}

/// UTF-8 text (optional BOM) into a table. `delimiter` overrides sniffing.
pub fn table_from_bytes(bytes: &[u8], name: &str, delimiter: Option<Delimiter>) -> Result<Table, String> {
    let text = decode_utf8(bytes, name)?;
    let delim = match delimiter {
        Some(d) => d,
        None => sniff(name, &text),
    };
    let raw = parse_delimited(&text, delim.char())?;
    let mut table = table_from_raw(&raw)?;
    table.delimiter = delim;
    Ok(table)
}

/// Grid rows whose first row is the header. Used for Excel and for tests.
pub fn table_from_grid(grid: Vec<Vec<String>>, delimiter: Delimiter) -> Result<Table, String> {
    if grid.len() > DATA_MERGE_MAX_ROWS.saturating_add(1) {
        return Err(format!("the file has more than {DATA_MERGE_MAX_ROWS} data rows"));
    }
    let raw = grid.into_iter().enumerate().map(|(i, cells)| RawRow { line: i + 1, cells }).collect::<Vec<_>>();
    let mut table = table_from_raw(&raw)?;
    table.delimiter = delimiter;
    Ok(table)
}

/// Inline `rows`: an array of objects. The first object's key order is the field list.
pub fn table_from_objects(rows: &[Value]) -> Result<Table, String> {
    let mut ordered = Vec::with_capacity(rows.len());
    for row in rows {
        let obj = row.as_object().ok_or_else(|| "each row must be an object".to_string())?;
        ordered.push(obj.iter().map(|(key, value)| (key.clone(), value.clone())).collect());
    }
    table_from_pairs(ordered)
}

/// A top-level JSON array of objects. Key order is the order in the text, not sorted order.
pub fn table_from_json(text: &str) -> Result<Table, String> {
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let rows: Vec<JsonRow> = serde_json::from_str(text).map_err(|err| err.to_string())?;
    table_from_pairs(rows.into_iter().map(|row| row.pairs).collect())
}

/// File bytes whose name is reported in a UTF-8 error. A leading byte-order mark is stripped.
pub fn table_from_json_bytes(bytes: &[u8], name: &str) -> Result<Table, String> {
    table_from_json(&decode_utf8(bytes, name)?)
}

/// One JSON object, with keys in the order they appear in the text.
struct JsonRow {
    pairs: Vec<(String, Value)>,
}

impl<'de> serde::Deserialize<'de> for JsonRow {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(JsonRowVisitor)
    }
}

struct JsonRowVisitor;

impl<'de> serde::de::Visitor<'de> for JsonRowVisitor {
    type Value = JsonRow;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a JSON object")
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut pairs = Vec::new();
        while let Some((key, value)) = map.next_entry::<String, Value>()? {
            if let Some(slot) = pairs.iter_mut().find(|(existing, _)| existing == &key) {
                slot.1 = value;
            } else {
                pairs.push((key, value));
            }
        }
        Ok(JsonRow { pairs })
    }
}

fn table_from_pairs(rows: Vec<Vec<(String, Value)>>) -> Result<Table, String> {
    let Some(first) = rows.first() else {
        return Err("no records".into());
    };
    if rows.len() > DATA_MERGE_MAX_ROWS {
        return Err(format!("the file has more than {DATA_MERGE_MAX_ROWS} data rows"));
    }
    let keys: Vec<String> = first.iter().map(|(key, _)| key.clone()).collect();
    if keys.len() > DATA_MERGE_MAX_COLS {
        return Err(format!("the file has {} columns; the limit is {DATA_MERGE_MAX_COLS}", keys.len()));
    }
    if keys.is_empty() {
        return Err("a field name is empty".into());
    }
    let mut fields = Vec::with_capacity(keys.len());
    let mut names = Vec::with_capacity(keys.len());
    for key in &keys {
        let (name, kind) = classify_header(key)?;
        if names.iter().any(|existing| existing == &name) {
            return Err(format!("duplicate field name \"{name}\""));
        }
        names.push(name.clone());
        fields.push(DataField { name, kind });
    }
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let mut cells = Vec::with_capacity(keys.len());
        for key in &keys {
            let value = row.iter().find(|(candidate, _)| candidate == key).map(|(_, value)| value);
            cells.push(cell_string(value)?);
        }
        for (key, _) in row {
            if !keys.iter().any(|candidate| candidate == key) {
                warnings.push(format!("Row {} has an extra field \"{key}\". It was ignored.", i + 1));
            }
        }
        if cells.iter().all(|cell| cell.is_empty()) {
            continue;
        }
        out.push(cells);
    }
    Ok(Table { fields, rows: out, warnings, delimiter: Delimiter::Comma })
}

pub(super) fn cell_string(v: Option<&Value>) -> Result<String, String> {
    Ok(match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(true)) => "true".into(),
        Some(Value::Bool(false)) => "false".into(),
        Some(Value::Array(_) | Value::Object(_)) => {
            return Err("a cell must be a string, number, bool, or null".into());
        }
    })
}

/// 0-based indexes into `count` rows, in first-seen order.
pub fn select_records(count: usize, records: &str, one: u32, range: &str, limit: Option<u32>) -> Result<Vec<usize>, String> {
    let mut idx = Vec::new();
    let mut seen = std::collections::HashSet::new();
    match records {
        "all" => idx.extend(0..count),
        "one" => {
            let i = record_index(one, count)?;
            idx.push(i);
        }
        "range" => {
            if range.trim().is_empty() {
                return Err("no records".into());
            }
            for token in range.split(',') {
                let token = token.trim();
                if token.is_empty() {
                    continue;
                }
                if let Some((a, b)) = token.split_once('-') {
                    let a = parse_record_no(a.trim())?;
                    let b = parse_record_no(b.trim())?;
                    if a > b {
                        return Err(format!("range {a}-{b} is reversed"));
                    }
                    let mut n = a;
                    while n <= b {
                        let i = record_index(n, count)?;
                        if seen.insert(i) {
                            idx.push(i);
                        }
                        n = n.saturating_add(1);
                    }
                } else {
                    let n = parse_record_no(token)?;
                    let i = record_index(n, count)?;
                    if seen.insert(i) {
                        idx.push(i);
                    }
                }
            }
        }
        other => return Err(format!("unknown records value \"{other}\"")),
    }
    if let Some(limit) = limit {
        if limit == 0 {
            return Err("limit must be at least 1".into());
        }
        idx.truncate(limit as usize);
    }
    if idx.is_empty() {
        return Err("no records".into());
    }
    Ok(idx)
}

fn parse_record_no(s: &str) -> Result<u32, String> {
    s.parse::<u32>().map_err(|_| format!("bad record \"{s}\""))
}

fn record_index(n: u32, count: usize) -> Result<usize, String> {
    if n == 0 || n as usize > count {
        return Err(format!("record {n} is out of range"));
    }
    Ok((n as usize) - 1)
}

/// Indexes of rows that survive `filter`, in file order.
pub fn filter_rows(fields: &[DataField], rows: &[Vec<String>], filter: &SourceFilter) -> Result<Vec<usize>, String> {
    if filter.rules.is_empty() {
        return Ok((0..rows.len()).collect());
    }
    if filter.match_mode != "all" && filter.match_mode != "any" {
        return Err(format!("unknown match \"{}\"", filter.match_mode));
    }
    for rule in &filter.rules {
        if !fields.iter().any(|field| field.name == rule.field) {
            return Err(format!("unknown field \"{}\"", rule.field));
        }
        if !known_op(&rule.op) {
            return Err(format!("unknown operator \"{}\"", rule.op));
        }
    }
    let mut out = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let hits = filter.rules.iter().filter(|rule| rule_hits(fields, row, rule)).count();
        let keep = if filter.match_mode == "any" { hits > 0 } else { hits == filter.rules.len() };
        if keep {
            out.push(i);
        }
    }
    Ok(out)
}

fn known_op(op: &str) -> bool {
    matches!(op, "equals" | "notEquals" | "contains" | "startsWith" | "empty" | "notEmpty")
}

fn rule_hits(fields: &[DataField], row: &[String], rule: &FilterRule) -> bool {
    let cell = cell_at(fields, row, &rule.field);
    match rule.op.as_str() {
        "equals" => cell == rule.value,
        "notEquals" => cell != rule.value,
        "contains" => cell.contains(&rule.value),
        "startsWith" => cell.starts_with(&rule.value),
        "empty" => cell.is_empty(),
        "notEmpty" => !cell.is_empty(),
        _ => false,
    }
}

/// Stable sort. Equal keys fall through to the next field, then to the original order.
pub fn sort_rows(fields: &[DataField], rows: &[Vec<String>], indices: &mut [usize], keys: &[SortKey]) -> Result<(), String> {
    if keys.is_empty() {
        return Ok(());
    }
    for key in keys {
        if !fields.iter().any(|field| field.name == key.field) {
            return Err(format!("unknown field \"{}\"", key.field));
        }
        if key.direction != "asc" && key.direction != "desc" {
            return Err(format!("unknown direction \"{}\"", key.direction));
        }
    }
    indices.sort_by(|&a, &b| {
        for key in keys {
            let left = cell_at(fields, rows.get(a).map(Vec::as_slice).unwrap_or(&[]), &key.field);
            let right = cell_at(fields, rows.get(b).map(Vec::as_slice).unwrap_or(&[]), &key.field);
            let mut ord = left.cmp(right);
            if key.direction == "desc" {
                ord = ord.reverse();
            }
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
        }
        std::cmp::Ordering::Equal
    });
    Ok(())
}

fn cell_at<'a>(fields: &[DataField], row: &'a [String], name: &str) -> &'a str {
    fields.iter().position(|field| field.name == name).and_then(|i| row.get(i)).map(String::as_str).unwrap_or("")
}

/// Slots for `count` copies of `block` on a page of the given size.
pub fn tile(input: &TileInput) -> TilePlan {
    let n = input.count;
    if n == 0 {
        return TilePlan { hits: Vec::new(), pages: 0, one_per_page: false };
    }
    let step_x = input.block.width() + input.column_spacing;
    let step_y = input.block.height() + input.row_spacing;
    let right = input.page_width - input.insets[1];
    let bottom = input.page_height - input.insets[2];
    let fits_col = |col: i32| step_x > 0.01 && input.block.x1 + f64::from(col) * step_x <= right + 0.01;
    let fits_row = |row: i32| step_y > 0.01 && input.block.y1 + f64::from(row) * step_y <= bottom + 0.01;
    let second_fits = fits_col(1) || fits_row(1);
    let mut hits = Vec::with_capacity(n);
    let push = |hits: &mut Vec<TileHit>, index: usize, page: usize, col: i32, row: i32| {
        hits.push(TileHit { index, page, col, row, dx: f64::from(col) * step_x, dy: f64::from(row) * step_y });
    };
    if n > 1 && !second_fits {
        for i in 0..n {
            push(&mut hits, i, i, 0, 0);
        }
        return TilePlan { hits, pages: n, one_per_page: true };
    }
    let mut col = 0i32;
    let mut row = 0i32;
    let mut page = 0usize;
    push(&mut hits, 0, 0, 0, 0);
    for i in 1..n {
        let (next_col, next_row, new_page) = next_slot(col, row, input.rows_first, &fits_col, &fits_row);
        if new_page {
            page = page.saturating_add(1);
            col = 0;
            row = 0;
        } else {
            col = next_col;
            row = next_row;
        }
        push(&mut hits, i, page, col, row);
    }
    TilePlan { hits, pages: page.saturating_add(1), one_per_page: false }
}

fn next_slot(col: i32, row: i32, rows_first: bool, fits_col: &impl Fn(i32) -> bool, fits_row: &impl Fn(i32) -> bool) -> (i32, i32, bool) {
    if rows_first {
        if fits_col(col.saturating_add(1)) {
            return (col.saturating_add(1), row, false);
        }
        if fits_row(row.saturating_add(1)) {
            return (0, row.saturating_add(1), false);
        }
    } else {
        if fits_row(row.saturating_add(1)) {
            return (col, row.saturating_add(1), false);
        }
        if fits_col(col.saturating_add(1)) {
            return (col.saturating_add(1), 0, false);
        }
    }
    (0, 0, true)
}

/// Files to try for an image cell. `http` and `https` are not candidates. Relative paths are
/// not resolved against the process working directory.
pub fn image_candidates(cell: &str, data_dir: Option<&Path>, doc_dir: Option<&Path>) -> Vec<PathBuf> {
    let cell = cell.trim();
    if cell.is_empty() {
        return Vec::new();
    }
    let lower = cell.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Vec::new();
    }
    let path = Path::new(cell);
    let mut out = Vec::new();
    let mut push = |p: PathBuf| {
        if !out.iter().any(|e| e == &p) {
            out.push(p);
        }
    };
    if path.is_absolute() {
        push(path.to_path_buf());
    }
    if let Some(dir) = data_dir {
        push(dir.join(cell));
    }
    if let Some(dir) = doc_dir {
        push(dir.join(cell));
    }
    out
}

/// `target` relative to `from_dir` (the document's folder).
pub fn relative_between(from_dir: &Path, target: &Path) -> String {
    let from: Vec<_> = from_dir.components().collect();
    let to: Vec<_> = target.components().collect();
    let mut i = 0;
    while i < from.len() && i < to.len() && from[i] == to[i] {
        i += 1;
    }
    let mut out = PathBuf::new();
    for _ in i..from.len() {
        out.push("..");
    }
    for c in to.iter().skip(i) {
        out.push(c);
    }
    if out.as_os_str().is_empty() {
        target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    } else {
        out.to_string_lossy().into_owned()
    }
}

pub fn fingerprint_of(path: &Path) -> Option<Fingerprint> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    Some(Fingerprint { size: meta.len(), mtime })
}

pub fn status_of(path: &Path, previous: Option<Fingerprint>) -> SourceStatus {
    let Some(now) = fingerprint_of(path) else {
        return SourceStatus::Missing;
    };
    match previous {
        Some(old) if old.size != now.size || old.mtime != now.mtime => SourceStatus::Modified,
        _ => SourceStatus::Ok,
    }
}

fn decode_utf8(bytes: &[u8], name: &str) -> Result<String, String> {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let mut text = String::from_utf8(bytes.to_vec()).map_err(|_| format!("{name} is not UTF-8"))?;
    if text.starts_with('\u{FEFF}') {
        text.replace_range(..'\u{FEFF}'.len_utf8(), "");
    }
    Ok(text)
}

fn sniff(name: &str, text: &str) -> Delimiter {
    let ext = extension(name);
    if ext == "tsv" || ext == "tab" {
        return Delimiter::Tab;
    }
    if ext == "csv" {
        return Delimiter::Comma;
    }
    let header = first_physical_line(text);
    if header.contains('\t') {
        Delimiter::Tab
    } else if header.contains(';') && !header.contains(',') {
        Delimiter::Semicolon
    } else {
        Delimiter::Comma
    }
}

pub fn extension(name: &str) -> String {
    Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

fn first_physical_line(text: &str) -> &str {
    text.split('\n').next().unwrap_or("").trim_end_matches('\r')
}

/// Header cell: trim, drop one leading apostrophe, then `@` is image and `#` is qr.
fn classify_header(raw: &str) -> Result<(String, DataFieldKind), String> {
    let trimmed = raw.trim();
    let rest = trimmed.strip_prefix('\'').unwrap_or(trimmed).trim();
    let (kind, name) = if let Some(n) = rest.strip_prefix('@') {
        (DataFieldKind::Image, n.trim())
    } else if let Some(n) = rest.strip_prefix('#') {
        (DataFieldKind::Qr, n.trim())
    } else {
        (DataFieldKind::Text, rest)
    };
    if name.is_empty() {
        return Err("a field name is empty".into());
    }
    Ok((name.to_string(), kind))
}

fn table_from_raw(raw: &[RawRow]) -> Result<Table, String> {
    let Some(header) = raw.first() else {
        return Err("the file has no header".into());
    };
    if header.cells.len() > DATA_MERGE_MAX_COLS {
        return Err(format!("the file has {} columns; the limit is {DATA_MERGE_MAX_COLS}", header.cells.len()));
    }
    if header.cells.is_empty() {
        return Err("a field name is empty".into());
    }
    let mut fields = Vec::with_capacity(header.cells.len());
    let mut names = Vec::with_capacity(header.cells.len());
    for cell in &header.cells {
        let (name, kind) = classify_header(cell)?;
        if names.iter().any(|n| n == &name) {
            return Err(format!("duplicate field name \"{name}\""));
        }
        names.push(name.clone());
        fields.push(DataField { name, kind });
    }
    let width = fields.len();
    let mut rows = Vec::new();
    let mut warnings = Vec::new();
    for raw_row in raw.iter().skip(1) {
        if raw_row.cells.iter().all(|c| c.trim().is_empty()) {
            continue;
        }
        if raw_row.cells.len() > width {
            warnings.push(format!("Row {} has cells past the header. Those cells were ignored.", raw_row.line));
        }
        let mut cells = Vec::with_capacity(width);
        for i in 0..width {
            cells.push(raw_row.cells.get(i).cloned().unwrap_or_default());
        }
        rows.push(cells);
        if rows.len() > DATA_MERGE_MAX_ROWS {
            return Err(format!("the file has more than {DATA_MERGE_MAX_ROWS} data rows"));
        }
    }
    Ok(Table { fields, rows, warnings, delimiter: Delimiter::Comma })
}

/// Split `s` on `delim`. Quoted fields keep commas, doubled quotes, and newlines.
/// More than 200 header columns, or the 100,001st non-empty data row, is an error and
/// the oversized row is not kept.
fn parse_delimited(s: &str, delim: char) -> Result<Vec<RawRow>, String> {
    let mut rows: Vec<RawRow> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut line = 1usize;
    let mut row_line = 1usize;
    let mut data_rows = 0usize;
    let mut header_width: Option<usize> = None;
    let mut discarding = false;
    let mut chars = s.chars().peekable();

    let finish = |row: &mut Vec<String>,
                  field: &mut String,
                  rows: &mut Vec<RawRow>,
                  data_rows: &mut usize,
                  header_width: &mut Option<usize>,
                  row_line: usize,
                  discarding: &mut bool|
     -> Result<(), String> {
        if !*discarding {
            row.push(std::mem::take(field));
        } else {
            field.clear();
        }
        *discarding = false;
        let empty = row.iter().all(|c| c.trim().is_empty());
        if empty {
            row.clear();
            return Ok(());
        }
        match *header_width {
            None => {
                if row.len() > DATA_MERGE_MAX_COLS {
                    return Err(format!("the file has {} columns; the limit is {DATA_MERGE_MAX_COLS}", row.len()));
                }
                *header_width = Some(row.len());
                rows.push(RawRow { line: row_line, cells: std::mem::take(row) });
            }
            Some(width) => {
                *data_rows = data_rows.saturating_add(1);
                if *data_rows > DATA_MERGE_MAX_ROWS {
                    return Err(format!("the file has more than {DATA_MERGE_MAX_ROWS} data rows"));
                }
                if row.len() > width.saturating_add(1) {
                    row.truncate(width.saturating_add(1));
                }
                rows.push(RawRow { line: row_line, cells: std::mem::take(row) });
            }
        }
        Ok(())
    };

    while let Some(c) = chars.next() {
        if c == '\n' {
            line = line.saturating_add(1);
        }
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
            continue;
        }
        if c == delim {
            if discarding {
                field.clear();
                continue;
            }
            row.push(std::mem::take(&mut field));
            if header_width.is_none() && row.len() > DATA_MERGE_MAX_COLS {
                return Err(format!("the file has {} columns; the limit is {DATA_MERGE_MAX_COLS}", row.len()));
            }
            if header_width.is_some_and(|w| row.len() > w.saturating_add(1)) {
                row.pop();
                discarding = true;
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !discarding => quoted = true,
            '\r' => {}
            '\n' => {
                finish(&mut row, &mut field, &mut rows, &mut data_rows, &mut header_width, row_line, &mut discarding)?;
                row_line = line;
            }
            _ if discarding => {}
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() || discarding {
        finish(&mut row, &mut field, &mut rows, &mut data_rows, &mut header_width, row_line, &mut discarding)?;
    }
    Ok(rows)
}
