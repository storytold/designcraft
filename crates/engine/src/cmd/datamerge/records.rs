//! The record list a preview and a merge walk.
//!
//! Each enabled source is filtered and sorted on its own. Those rows are then concatenated, or
//! joined onto one driving source. A document-level sort runs after that, and range and limit
//! run after the sort. The merge index is the 1-based position in that chosen list. Skip-on-warning
//! drops records later and does not renumber them.

use std::path::{Path, PathBuf};

use designcraft_doc::{AffixRule, DataField, DataSource, Document, MERGE_INDEX_FIELD, MergeOptions, SOURCE_FILENAME_FIELD, SortKey, SourceStatus};

use super::parse::{filter_rows, select_records, sort_rows, status_of};

/// One source's contribution to an output record.
#[derive(Clone, Debug)]
pub struct RecordPart {
    pub source_id: Option<u64>,
    pub fields: Vec<DataField>,
    pub cells: Vec<String>,
    pub data_dir: Option<PathBuf>,
    pub affixes: Vec<AffixRule>,
    pub filename: String,
    /// An unmatched join contributes a blank part and does not warn.
    pub matched: bool,
}

/// One output record. `number` is the merge index.
#[derive(Clone, Debug)]
pub struct CellRecord {
    pub parts: Vec<RecordPart>,
    pub number: u32,
}

impl CellRecord {
    pub fn blank() -> Self {
        CellRecord { parts: Vec::new(), number: 0 }
    }
}

pub fn source_filename(src: &DataSource) -> String {
    if let Some(path) = &src.path {
        let name = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if !name.is_empty() {
            return name;
        }
    }
    src.name.clone()
}

fn data_dir(src: &DataSource) -> Option<PathBuf> {
    let path = src.path.as_deref()?;
    let parent = Path::new(path).parent()?;
    if parent.as_os_str().is_empty() { None } else { Some(parent.to_path_buf()) }
}

pub fn part_from_row(src: &DataSource, cells: Vec<String>) -> RecordPart {
    RecordPart {
        source_id: Some(src.id),
        fields: src.fields.clone(),
        cells,
        data_dir: data_dir(src),
        affixes: src.affixes.clone(),
        filename: source_filename(src),
        matched: true,
    }
}

fn empty_part(src: &DataSource) -> RecordPart {
    RecordPart {
        source_id: Some(src.id),
        fields: src.fields.clone(),
        cells: vec![String::new(); src.fields.len()],
        data_dir: data_dir(src),
        affixes: src.affixes.clone(),
        filename: source_filename(src),
        matched: false,
    }
}

pub fn stored_cell<'a>(part: &'a RecordPart, field: &str) -> &'a str {
    part.fields.iter().position(|f| f.name == field).and_then(|i| part.cells.get(i)).map(String::as_str).unwrap_or("")
}

pub fn part_has_field(part: &RecordPart, field: &str) -> bool {
    part.fields.iter().any(|f| f.name == field)
}

/// Text a placeholder sees. A real column wins over a virtual field of the same name.
pub fn resolve_field(part: &RecordPart, number: u32, field: &str) -> String {
    let raw = if part_has_field(part, field) {
        stored_cell(part, field).to_string()
    } else if field == MERGE_INDEX_FIELD {
        number.to_string()
    } else if field == SOURCE_FILENAME_FIELD {
        part.filename.clone()
    } else {
        String::new()
    };
    apply_affixes(&part.affixes, field, &raw)
}

fn apply_affixes(rules: &[AffixRule], field: &str, raw: &str) -> String {
    let mut out = raw.to_string();
    for rule in rules {
        if rule.field != field {
            continue;
        }
        if rule.non_empty && raw.is_empty() {
            continue;
        }
        match rule.place.as_str() {
            "prefix" => out = format!("{}{out}", rule.text),
            "postfix" => out.push_str(&rule.text),
            _ => {}
        }
    }
    out
}

/// First part that has the field, which is the driving part when several do. A missing field is empty.
pub fn sort_value(rec: &CellRecord, field: &str) -> String {
    for part in &rec.parts {
        if part_has_field(part, field) {
            return stored_cell(part, field).to_string();
        }
    }
    String::new()
}

pub fn global_sort(records: &mut [CellRecord], keys: &[SortKey]) -> Result<(), String> {
    if keys.is_empty() {
        return Ok(());
    }
    for key in keys {
        if key.direction != "asc" && key.direction != "desc" {
            return Err(format!("unknown direction \"{}\"", key.direction));
        }
    }
    records.sort_by(|left, right| {
        for key in keys {
            let mut ord = sort_value(left, &key.field).cmp(&sort_value(right, &key.field));
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

struct Kept<'a> {
    src: &'a DataSource,
    parts: Vec<RecordPart>,
}

/// Filtered, per-source sorted, joined or concatenated, then globally sorted. Numbers are still 0.
pub fn linked_records(doc: &Document) -> Result<(Vec<CellRecord>, Vec<String>, Vec<(u64, SourceStatus)>), String> {
    if doc.data_merge.sources.is_empty() {
        return Err("select a data source or pass csv or rows".into());
    }
    let mut warnings = Vec::new();
    let mut statuses = Vec::new();
    for src in &doc.data_merge.sources {
        let status = if let Some(path) = &src.path {
            let status = status_of(Path::new(path), src.fingerprint);
            match status {
                SourceStatus::Modified => warnings.push("The data file changed since it was read. This merge used the cached rows.".into()),
                SourceStatus::Missing => warnings.push("The data file is missing. This merge used the cached rows.".into()),
                SourceStatus::Ok => {}
            }
            status
        } else {
            src.status
        };
        statuses.push((src.id, status));
    }
    if let Some(id) = doc.data_merge.driving {
        let Some(src) = doc.data_merge.sources.iter().find(|src| src.id == id) else {
            return Err(format!("unknown driving source {id}"));
        };
        if !src.enabled {
            return Err("the driving source is not enabled".into());
        }
    }
    let mut any_enabled = false;
    let mut kept = Vec::new();
    for src in &doc.data_merge.sources {
        if !src.enabled {
            continue;
        }
        any_enabled = true;
        let mut order = filter_rows(&src.fields, &src.rows, &src.filter)?;
        sort_rows(&src.fields, &src.rows, &mut order, &src.sort)?;
        warnings.extend(src.warnings.iter().cloned());
        let parts = order.into_iter().map(|i| part_from_row(src, src.rows.get(i).cloned().unwrap_or_default())).collect();
        kept.push(Kept { src, parts });
    }
    if !any_enabled {
        return Err("no enabled data source".into());
    }
    let mut records = if let Some(id) = doc.data_merge.driving { join_records(&kept, id, &doc.data_merge.joins)? } else { concat_records(&kept) };
    global_sort(&mut records, &doc.data_merge.sort)?;
    if records.is_empty() {
        return Err("no records".into());
    }
    Ok((records, warnings, statuses))
}

fn concat_records(kept: &[Kept<'_>]) -> Vec<CellRecord> {
    let mut records = Vec::new();
    for source in kept {
        for part in &source.parts {
            records.push(CellRecord { parts: vec![part.clone()], number: 0 });
        }
    }
    records
}

fn join_records(kept: &[Kept<'_>], driving: u64, links: &[designcraft_doc::JoinLink]) -> Result<Vec<CellRecord>, String> {
    let drive = kept.iter().find(|k| k.src.id == driving).ok_or_else(|| format!("unknown driving source {driving}"))?;
    let mut others = Vec::new();
    for source in kept {
        if source.src.id == driving {
            continue;
        }
        let Some(link) = links.iter().find(|link| link.source == source.src.id) else {
            return Err(format!("source {} has no join key", source.src.id));
        };
        if !part_field_on_source(drive.src, &link.driving_field) {
            return Err(format!("unknown field \"{}\"", link.driving_field));
        }
        if !part_field_on_source(source.src, &link.field) {
            return Err(format!("unknown field \"{}\"", link.field));
        }
        others.push((source, link));
    }
    let mut records = Vec::new();
    for (i, part) in drive.parts.iter().enumerate() {
        let drive_index = u32::try_from(i.saturating_add(1)).unwrap_or(u32::MAX);
        let mut parts = vec![part.clone()];
        for (source, link) in &others {
            let want = key_text(part, &link.driving_field, drive_index);
            let found = source.parts.iter().enumerate().find(|(j, other)| {
                let idx = u32::try_from(j.saturating_add(1)).unwrap_or(u32::MAX);
                key_text(other, &link.field, idx) == want
            });
            parts.push(found.map(|(_, other)| other.clone()).unwrap_or_else(|| empty_part(source.src)));
        }
        records.push(CellRecord { parts, number: 0 });
    }
    Ok(records)
}

fn key_text(part: &RecordPart, field: &str, index_in_source: u32) -> String {
    if part_has_field(part, field) {
        return stored_cell(part, field).to_string();
    }
    if field == SOURCE_FILENAME_FIELD {
        return part.filename.clone();
    }
    if field == MERGE_INDEX_FIELD {
        return index_in_source.to_string();
    }
    String::new()
}

fn part_field_on_source(src: &DataSource, field: &str) -> bool {
    src.fields.iter().any(|f| f.name == field) || field == SOURCE_FILENAME_FIELD || field == MERGE_INDEX_FIELD
}

/// Apply range and limit, then assign merge indexes. An empty selection is an error.
pub fn choose(records: &[CellRecord], options: &MergeOptions) -> Result<Vec<CellRecord>, String> {
    let idx = select_records(records.len(), &options.records, options.one, &options.range, options.limit)?;
    let mut chosen = Vec::with_capacity(idx.len());
    for (n, i) in idx.into_iter().enumerate() {
        let Some(mut rec) = records.get(i).cloned() else { continue };
        rec.number = u32::try_from(n.saturating_add(1)).unwrap_or(u32::MAX);
        chosen.push(rec);
    }
    if chosen.is_empty() {
        return Err("no records".into());
    }
    Ok(chosen)
}

pub fn inline_records(fields: Vec<DataField>, rows: Vec<Vec<String>>, data_dir: Option<PathBuf>, filename: String) -> Vec<CellRecord> {
    rows.into_iter()
        .map(|cells| CellRecord {
            parts: vec![RecordPart {
                source_id: None,
                fields: fields.clone(),
                cells,
                data_dir: data_dir.clone(),
                affixes: Vec::new(),
                filename: filename.clone(),
                matched: true,
            }],
            number: 0,
        })
        .collect()
}
