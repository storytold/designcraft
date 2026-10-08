//! Data merge commands: linked tables, placeholders, preview, and a new merged document.
//!
//! `data.merge` leaves the template and its undo stack alone and appends a new document.
//! A drawn grid repeats its prototype and is removed from that new document.

mod fill;
mod grid;
mod parse;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use designcraft_doc::{
    DataField, DataFieldKind, DataSource, Delimiter, Document, FilterRule, HyperlinkSource, ItemId, MergeOptions, Page, Placeholder,
    PlaceholderAnchor, PlaceholderRole, Selection, SortKey, SourceFilter, SourceStatus, SpreadRef, StoryId,
};
use designcraft_geom::{Rect, Vec2};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, id_param, ok, str_param};
use crate::{DocState, Result, Session};
use fill::{FillMode, FillReport, Maps, MissingImage, copy_item, fill_row, identity_map};
use grid::CellRecord;
use parse::{
    Table, TileInput, extension, filter_rows, fingerprint_of, relative_between, select_records, sort_rows, status_of, table_from_bytes,
    table_from_grid, table_from_json, table_from_json_bytes, table_from_objects, tile,
};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "data.source.select",
            "Select Data Source…",
            [],
            None,
            "{path | bytes|base64, name | json | csv | rows, id?, delimiter?: comma|tab|semicolon, sheet?} → {id, fields, records, warnings}",
            has_doc,
            source_select
        ),
        cmd!(
            "data.source.update",
            "Update Data Source",
            [],
            None,
            "{id?} → re-read one linked file and keep its id, enabled flag, filter, and sort",
            has_doc,
            source_update
        ),
        cmd!("data.source.remove", "Remove Data Source", [], None, "{id?} → drop that source; placeholders stay", has_doc, source_remove),
        cmd!("data.source.enabled", "Enable Data Source", [], None, "{id?, enabled} → include or skip that source", has_doc, source_enabled),
        cmd!("data.source.filter", "Filter Data Source", [], None, "{id?, match?: all|any, rules?: [{field, op, value?}]}", has_doc, source_filter),
        cmd!("data.source.sort", "Sort Data Source", [], None, "{id?, fields?: [{field, direction?: asc|desc}]}", has_doc, source_sort),
        cmd!(
            query "data.fields",
            "Data Fields",
            [],
            None,
            "{csv? | rows? | json? | path? | bytes?, name?} → [{name, kind, uses, sourceId, sourceName, enabled, records}]; a payload is parsed and not attached",
            has_doc,
            fields_cmd
        ),
        cmd!(
            "data.placeholder.add",
            "Insert Field",
            [],
            None,
            "{field, source?, role?: text|image|qr|hyperlink, story?, at?, end?, item?} → {id}",
            has_doc,
            placeholder_add
        ),
        cmd!("data.placeholder.remove", "Remove Field", [], None, "{id}", has_doc, placeholder_remove),
        cmd!(
            "data.options",
            "Data Merge Options",
            [],
            None,
            "{records?, one?, range?, perPage?, arrange?, insets?, columnSpacing?, rowSpacing?, fitting?, center?, linkImages?, limit?}",
            has_doc,
            options_cmd
        ),
        cmd!(query "data.preview", "Preview Record", [], None, "{record?: n} → fill the template with that record (session only)", has_doc, preview_cmd),
        cmd!(noundo "data.preview.stop", "Stop Preview", [], None, "{} → restore the unfilled template", has_doc, |_s, _| ok()),
        cmd!(
            "data.merge",
            "Create Merged Document…",
            [],
            None,
            "{csv? | rows? | json? | path? | bytes?, name?, records?, one?, range?, perPage?, arrange?, insets?, columnSpacing?, rowSpacing?, fitting?, center?, linkImages?, limit?} → {records, pages, missingImages, oversetStories, warnings}",
            has_doc,
            merge_cmd
        ),
        cmd!(
            "data.grid.create",
            "Create Grid",
            [],
            None,
            "{rect, spread?, rows?, columns?, gutter?, recordOffset?, recordAdvance?, origin?, arrange?} → {id}",
            has_doc,
            grid::create
        ),
        cmd!(
            "data.grid.set",
            "Set Grid",
            [],
            None,
            "{id?, rows?, columns?, gutter?, recordOffset?, recordAdvance?, origin?, arrange?}",
            has_doc,
            grid::set
        ),
        cmd!("data.grid.adopt", "Adopt Into Grid", [], None, "{id?} → parent the selection into that grid's origin cell", has_doc, grid::adopt),
        cmd!(
            "data.grid.release",
            "Release Grid",
            [],
            None,
            "{id?} → put that grid's children back on the page and delete the grid",
            has_doc,
            grid::release
        ),
    ]
}

fn source_select(s: &mut Session, p: &Value) -> Result<Value> {
    let (mut source, mut report) = read_source(p).map_err(|e| bad("data.source.select", e))?;
    let replace = p.get("id").and_then(Value::as_u64);
    s.edit(move |d, _| {
        let id = if let Some(id) = replace {
            let Some(pos) = d.data_merge.sources.iter().position(|src| src.id == id) else {
                return Err(bad("data.source.select", format!("unknown data source {id}")));
            };
            source.id = id;
            source.enabled = d.data_merge.sources[pos].enabled;
            source.filter = d.data_merge.sources[pos].filter.clone();
            source.sort = d.data_merge.sources[pos].sort.clone();
            d.data_merge.sources[pos] = source;
            id
        } else {
            let id = d.alloc();
            source.id = id;
            d.data_merge.sources.push(source);
            id
        };
        if let Some(obj) = report.as_object_mut() {
            obj.insert("id".to_string(), json!(id));
        }
        Ok(report)
    })
}

fn source_update(s: &mut Session, p: &Value) -> Result<Value> {
    let requested = p.get("id").and_then(Value::as_u64);
    let (path, name, delimiter, sheet) = {
        let doc = &s.doc()?.doc;
        let idx = source_index(&doc.data_merge.sources, requested).map_err(|e| bad("data.source.update", e))?;
        let src = &doc.data_merge.sources[idx];
        let path = src.path.clone().ok_or_else(|| bad("data.source.update", "this source has no file to update"))?;
        (path, src.name.clone(), src.delimiter, src.sheet.clone())
    };
    let bytes = read_path(&path).map_err(|e| bad("data.source.update", e))?;
    let table = parse_file(&bytes, &name, Some(delimiter), sheet.as_deref()).map_err(|e| bad("data.source.update", e))?;
    let report = json!({
        "fields": field_pairs(&table.fields),
        "records": table.rows.len(),
        "warnings": table.warnings.clone(),
    });
    s.edit(move |d, _| {
        let idx = source_index(&d.data_merge.sources, requested).map_err(|e| bad("data.source.update", e))?;
        let src = &mut d.data_merge.sources[idx];
        src.fields = table.fields;
        src.rows = table.rows;
        src.warnings = table.warnings;
        src.delimiter = table.delimiter;
        src.fingerprint = fingerprint_of(Path::new(&path));
        src.status = SourceStatus::Ok;
        Ok(report)
    })
}

fn source_remove(s: &mut Session, p: &Value) -> Result<Value> {
    let requested = p.get("id").and_then(Value::as_u64);
    s.edit(move |d, _| {
        let idx = source_index(&d.data_merge.sources, requested).map_err(|e| bad("data.source.remove", e))?;
        d.data_merge.sources.remove(idx);
        ok()
    })
}

fn source_enabled(s: &mut Session, p: &Value) -> Result<Value> {
    let requested = p.get("id").and_then(Value::as_u64);
    let enabled = p.get("enabled").and_then(Value::as_bool).ok_or_else(|| bad("data.source.enabled", "missing `enabled`"))?;
    s.edit(move |d, _| {
        let idx = source_index(&d.data_merge.sources, requested).map_err(|e| bad("data.source.enabled", e))?;
        d.data_merge.sources[idx].enabled = enabled;
        ok()
    })
}

fn source_filter(s: &mut Session, p: &Value) -> Result<Value> {
    let requested = p.get("id").and_then(Value::as_u64);
    let match_mode = str_param(p, "match").unwrap_or("all").to_string();
    if match_mode != "all" && match_mode != "any" {
        return Err(bad("data.source.filter", format!("unknown match \"{match_mode}\"")));
    }
    let rules = parse_rules(p.get("rules")).map_err(|e| bad("data.source.filter", e))?;
    s.edit(move |d, _| {
        let idx = source_index(&d.data_merge.sources, requested).map_err(|e| bad("data.source.filter", e))?;
        for rule in &rules {
            if !d.data_merge.sources[idx].fields.iter().any(|field| field.name == rule.field) {
                return Err(bad("data.source.filter", format!("unknown field \"{}\"", rule.field)));
            }
        }
        d.data_merge.sources[idx].filter = SourceFilter { match_mode, rules };
        ok()
    })
}

fn source_sort(s: &mut Session, p: &Value) -> Result<Value> {
    let requested = p.get("id").and_then(Value::as_u64);
    let keys = parse_sort(p.get("fields")).map_err(|e| bad("data.source.sort", e))?;
    s.edit(move |d, _| {
        let idx = source_index(&d.data_merge.sources, requested).map_err(|e| bad("data.source.sort", e))?;
        for key in &keys {
            if !d.data_merge.sources[idx].fields.iter().any(|field| field.name == key.field) {
                return Err(bad("data.source.sort", format!("unknown field \"{}\"", key.field)));
            }
        }
        d.data_merge.sources[idx].sort = keys;
        ok()
    })
}

fn fields_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    if payload_present(p) {
        let (table, _) = load_inline(p).map_err(|e| bad("data.fields", e))?;
        let name = str_param(p, "name").unwrap_or("");
        let entries: Vec<Value> = table
            .fields
            .iter()
            .map(|field| {
                json!({
                    "name": field.name,
                    "kind": field.kind.as_str(),
                    "uses": 0,
                    "sourceId": Value::Null,
                    "sourceName": name,
                    "enabled": true,
                    "records": table.rows.len(),
                })
            })
            .collect();
        return Ok(Value::Array(entries));
    }
    let st = s.doc()?;
    let doc = &st.doc;
    let preview = preview_len(doc);
    let mut entries = Vec::new();
    for src in &doc.data_merge.sources {
        let records = filter_rows(&src.fields, &src.rows, &src.filter).map(|kept| kept.len()).unwrap_or(0);
        for field in &src.fields {
            let uses = doc.data_merge.placeholders.iter().filter(|ph| ph.source_id == src.id && ph.field == field.name).count();
            entries.push(json!({
                "name": field.name,
                "kind": field.kind.as_str(),
                "uses": uses,
                "sourceId": src.id,
                "sourceName": src.name,
                "enabled": src.enabled,
                "records": records,
                "preview": preview,
            }));
        }
    }
    Ok(Value::Array(entries))
}

fn preview_len(doc: &Document) -> usize {
    let Ok((records, _, _)) = prepare_linked(doc) else {
        return 0;
    };
    let options = &doc.data_merge.options;
    select_records(records.len(), &options.records, options.one, &options.range, options.limit).map(|idx| idx.len()).unwrap_or(0)
}

fn placeholder_add(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "field").ok_or_else(|| bad("data.placeholder.add", "missing `field`"))?.to_string();
    s.edit(|d, sel| add_placeholder(d, sel, p, &name))
}

fn placeholder_remove(s: &mut Session, p: &Value) -> Result<Value> {
    let id = p.get("id").and_then(Value::as_u64).ok_or_else(|| bad("data.placeholder.remove", "missing `id`"))?;
    s.edit(move |d, _| {
        let Some(ph) = d.data_merge.placeholders.iter().find(|ph| ph.id == id).cloned() else {
            return Err(bad("data.placeholder.remove", "no placeholder"));
        };
        if let PlaceholderAnchor::Text { story, start, end } = ph.anchor {
            let marker = format!("<<{}>>", ph.field);
            let matches = d.story(story).and_then(|st| st.text.get(start..end)).is_some_and(|t| t == marker);
            if matches {
                if let Some(st) = d.story_mut(story) {
                    let end = end.min(st.text.len());
                    let start = start.min(end);
                    st.replace(start..end, "");
                }
                let removed = end.saturating_sub(start);
                shift_after(d, story, end, -(removed as isize));
            }
        }
        d.data_merge.placeholders.retain(|ph| ph.id != id);
        ok()
    })
}

fn options_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    s.edit(|d, sel| {
        apply_options(&mut d.data_merge.options, p).map_err(|e| bad("data.options", e))?;
        validate(&d.data_merge.options).map_err(|e| bad("data.options", e))?;
        if d.data_merge.options.per_page == "multiple" {
            let (removed, ids) = grid::release_all(d).map_err(|e| bad("data.options", e))?;
            if removed > 0 {
                *sel = Selection::items(ids);
            }
        }
        serde_json::to_value(&d.data_merge.options).map_err(|e| bad("data.options", e.to_string()))
    })
}

fn preview_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let (records, options, doc_dir, grids) = {
        let st = s.doc()?;
        let (full, _, _) = prepare_linked(&st.doc).map_err(|e| bad("data.preview", e))?;
        let options = st.doc.data_merge.options.clone();
        let selected =
            select_records(full.len(), &options.records, options.one, &options.range, options.limit).map_err(|e| bad("data.preview", e))?;
        let records: Vec<CellRecord> = selected.iter().filter_map(|idx| full.get(*idx).cloned()).collect();
        if records.is_empty() {
            return Err(bad("data.preview", "no records"));
        }
        let grids = grid::grids_for_merge(&st.doc, &options.per_page).map_err(|e| bad("data.preview", e))?;
        let doc_dir = st.path.as_deref().and_then(parent_dir);
        (records, options, doc_dir, grids)
    };
    let max = records.len() as u64;
    let record = p.get("record").and_then(Value::as_u64).unwrap_or(1).clamp(1, max);
    let doc = {
        let st = s.doc_mut()?;
        let base = if let Some(stash) = &st.preview_stash {
            stash.clone()
        } else {
            let current = st.doc.clone();
            st.preview_stash = Some(current.clone());
            current
        };
        let mut doc = (*base).clone();
        let start = (record as usize).saturating_sub(1);
        if grids.is_empty() {
            if let Some(rec) = records.get(start) {
                let maps = identity_map(&doc);
                let mode = match rec.source_id {
                    Some(id) => FillMode::Source(id),
                    None => FillMode::Named,
                };
                fill_row(&mut doc, &rec.fields, &rec.cells, rec.number, &options, &maps, rec.data_dir.as_deref(), doc_dir.as_deref(), mode);
            }
        } else {
            let visits = grid::plan_preview(start, &grids, records.len());
            let maps = identity_map(&doc);
            let mut warnings = Vec::new();
            let mut missing = Vec::new();
            let mut parent = false;
            grid::explode_cycle(
                &mut doc,
                &maps,
                &records,
                &visits,
                Some(start),
                &options,
                doc_dir.as_deref(),
                &mut warnings,
                &mut missing,
                &mut parent,
            )?;
        }
        doc
    };
    let st = s.doc_mut()?;
    st.doc = Arc::new(doc);
    st.preview_record = Some(record as u32);
    st.revision = st.revision.saturating_add(1);
    Ok(json!({"record": record, "count": max}))
}

fn merge_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    if p.get("spread").is_some() {
        return Err(bad("data.merge", "the spread parameter has been removed"));
    }
    // A merged document has no fields left. Another click merges its template again.
    use_merge_template(s)?;
    let (mut warnings, doc_dir, options, chosen, src, statuses, grids) = {
        let st = s.doc()?;
        let (full, status_warnings, statuses) = if payload_present(p) {
            let (table, dir) = load_inline(p).map_err(|e| bad("data.merge", e))?;
            let (records, warnings) = records_from_table(table, dir);
            (records, warnings, Vec::new())
        } else {
            prepare_linked(&st.doc).map_err(|e| bad("data.merge", e))?
        };
        let mut options = st.doc.data_merge.options.clone();
        apply_options(&mut options, p).map_err(|e| bad("data.merge", e))?;
        validate(&options).map_err(|e| bad("data.merge", e))?;
        let grids = grid::grids_for_merge(&st.doc, &options.per_page).map_err(|e| bad("data.merge", e))?;
        let selected = select_records(full.len(), &options.records, options.one, &options.range, options.limit).map_err(|e| bad("data.merge", e))?;
        if grids.is_empty() && options.per_page == "multiple" && (st.doc.settings.facing_pages || st.doc.page_count() != 1) {
            return Err(bad("data.merge", "multiple records on a page need one page with facing pages off"));
        }
        if !grids.is_empty() {
            grid::plan_cycles(selected.len(), &grids).map_err(|e| bad("data.merge", e))?;
        }
        let chosen: Vec<CellRecord> = selected.iter().filter_map(|idx| full.get(*idx).cloned()).collect();
        let doc_dir = st.path.as_deref().and_then(parent_dir);
        let src = (*st.doc).clone();
        (status_warnings, doc_dir, options, chosen, src, statuses, grids)
    };
    let mut merged = src.clone();
    let mut missing = Vec::new();
    if !grids.is_empty() {
        let cycles = grid::plan_cycles(chosen.len(), &grids).map_err(|e| bad("data.merge", e))?;
        let mut parent = false;
        for (ci, visits) in cycles.iter().enumerate() {
            let maps = if ci == 0 { identity_map(&merged) } else { append_copy(&mut merged, &src)? };
            let origin = visits.first().and_then(|visit| visit.cells.first().copied()).flatten();
            grid::explode_cycle(&mut merged, &maps, &chosen, visits, origin, &options, doc_dir.as_deref(), &mut warnings, &mut missing, &mut parent)?;
        }
    } else if options.per_page == "multiple" {
        tile_records(&mut merged, &src, &chosen, &options, doc_dir.as_deref(), &mut warnings, &mut missing)?;
    } else {
        copy_records(&mut merged, &src, &chosen, &options, doc_dir.as_deref(), &mut warnings, &mut missing)?;
    }
    // Status is written only after every error return. The new document then becomes active,
    // so this template edit does not push an undo step. The fingerprint stays the last read.
    if !statuses.is_empty() {
        remember_statuses(s, &statuses);
        apply_statuses(&mut merged, &statuses);
    }
    merged.data_merge.placeholders.clear();
    merged.data_merge.template_uid = s.active().map(|st| st.uid);
    merged.title = format!("{} merged", src.title);
    let pages = merged.page_count();
    let overset = overset_count(&merged);
    let records = chosen.len();
    let missing_json: Vec<Value> = missing.iter().map(|m| json!({"record": m.record, "field": m.field, "path": m.path})).collect();
    s.add_document(DocState::new(merged, None));
    Ok(json!({
        "records": records,
        "pages": pages,
        "missingImages": missing_json,
        "oversetStories": overset,
        "warnings": warnings,
    }))
}

fn copy_records(
    dst: &mut Document,
    src: &Document,
    records: &[CellRecord],
    options: &MergeOptions,
    doc_dir: Option<&Path>,
    warnings: &mut Vec<String>,
    missing: &mut Vec<MissingImage>,
) -> Result<()> {
    let mut parent = false;
    if let Some(rec) = records.first() {
        let maps = identity_map(dst);
        fill_record(dst, rec, options, &maps, doc_dir, warnings, missing, &mut parent);
    }
    for rec in records.iter().skip(1) {
        let maps = append_copy(dst, src)?;
        fill_record(dst, rec, options, &maps, doc_dir, warnings, missing, &mut parent);
    }
    Ok(())
}

fn append_copy(dst: &mut Document, src: &Document) -> Result<Maps> {
    let start = dst.page_count();
    let n = src.page_count().max(1);
    let parent = src.page(0).and_then(|p| p.parent);
    dst.insert_pages(Some(start.saturating_sub(1)), n, parent)?;
    for i in 0..n {
        if let Some(pg) = src.page(i).cloned() {
            assign_page(dst, start.saturating_add(i), &pg);
        }
    }
    let mut maps = Maps::default();
    for (spi, sp) in src.spreads.iter().enumerate() {
        for it in &sp.items {
            let pi = sp.page_at_x(it.bounds().center().x).unwrap_or(0);
            let abs = src.first_page_of_spread(spi).saturating_add(pi);
            let dest_abs = start.saturating_add(abs);
            let Some((dsi, _)) = dst.page_loc(dest_abs) else { continue };
            let src_x = src.page(abs).map(|p| p.x).unwrap_or(0.0);
            let dst_x = dst.page(dest_abs).map(|p| p.x).unwrap_or(0.0);
            copy_item(dst, src, it, SpreadRef::Doc(dsi), Vec2::new(dst_x - src_x, 0.0), &mut maps)?;
        }
    }
    Ok(maps)
}

fn tile_records(
    dst: &mut Document,
    src: &Document,
    records: &[CellRecord],
    options: &MergeOptions,
    doc_dir: Option<&Path>,
    warnings: &mut Vec<String>,
    missing: &mut Vec<MissingImage>,
) -> Result<()> {
    let (block, ids) = block_of(src);
    let (width, height) = src.page(0).map(|p| (p.width, p.height)).unwrap_or((612.0, 792.0));
    let plan = tile(&TileInput {
        block,
        page_width: width,
        page_height: height,
        insets: options.insets,
        column_spacing: options.column_spacing,
        row_spacing: options.row_spacing,
        rows_first: options.arrange != "columns",
        count: records.len(),
    });
    if plan.one_per_page {
        warnings.push("Only one record fits on the page, so each record has its own page.".into());
    }
    if plan.pages > 1 {
        let parent = src.page(0).and_then(|p| p.parent);
        let template = src.page(0).cloned();
        dst.insert_pages(Some(0), plan.pages.saturating_sub(1), parent)?;
        if let Some(pg) = template {
            for i in 1..plan.pages {
                assign_page(dst, i, &pg);
            }
        }
    }
    let mut parent = false;
    for hit in plan.hits {
        let Some(rec) = records.get(hit.index) else { continue };
        if hit.index == 0 && hit.page == 0 && hit.col == 0 && hit.row == 0 {
            let maps = identity_map(dst);
            fill_record(dst, rec, options, &maps, doc_dir, warnings, missing, &mut parent);
            continue;
        }
        let Some((dsi, _)) = dst.page_loc(hit.page) else { continue };
        let mut maps = Maps::default();
        for id in &ids {
            let Some(it) = src.item(*id) else { continue };
            copy_item(dst, src, it, SpreadRef::Doc(dsi), Vec2::new(hit.dx, hit.dy), &mut maps)?;
        }
        fill_record(dst, rec, options, &maps, doc_dir, warnings, missing, &mut parent);
    }
    Ok(())
}

fn fill_record(
    doc: &mut Document,
    rec: &CellRecord,
    options: &MergeOptions,
    maps: &Maps,
    doc_dir: Option<&Path>,
    warnings: &mut Vec<String>,
    missing: &mut Vec<MissingImage>,
    parent: &mut bool,
) {
    let mode = match rec.source_id {
        Some(id) => FillMode::Source(id),
        None => FillMode::Named,
    };
    let report = fill_row(doc, &rec.fields, &rec.cells, rec.number, options, maps, rec.data_dir.as_deref(), doc_dir, mode);
    absorb(report, warnings, missing, parent);
}

fn absorb(report: FillReport, warnings: &mut Vec<String>, missing: &mut Vec<MissingImage>, parent: &mut bool) {
    for w in report.warnings {
        if w == fill::PARENT_WARNING {
            if *parent {
                continue;
            }
            *parent = true;
        }
        warnings.push(w);
    }
    missing.extend(report.missing);
}

fn block_of(doc: &Document) -> (Rect, Vec<ItemId>) {
    let Some(page) = doc.page(0) else {
        return (Rect::ZERO, Vec::new());
    };
    let pb = page.bounds();
    let mut block: Option<Rect> = None;
    let mut ids = Vec::new();
    for sp in &doc.spreads {
        for it in &sp.items {
            let b = it.bounds();
            if b.x0 < pb.x1 && b.x1 > pb.x0 && b.y0 < pb.y1 && b.y1 > pb.y0 {
                ids.push(it.id);
                block = Some(match block {
                    Some(u) => u.union(b),
                    None => b,
                });
            }
        }
    }
    (block.unwrap_or(Rect::ZERO), ids)
}

fn assign_page(dst: &mut Document, abs: usize, from: &Page) {
    let Some((si, pi)) = dst.page_loc(abs) else { return };
    let sp = Arc::make_mut(&mut dst.spreads[si]);
    let Some(p) = sp.pages.get_mut(pi) else { return };
    let size_changed = (p.width - from.width).abs() > 0.01 || (p.height - from.height).abs() > 0.01;
    p.guides = from.guides.clone();
    p.parent = from.parent;
    p.margins = from.margins;
    p.columns = from.columns.clone();
    p.width = from.width;
    p.height = from.height;
    if size_changed {
        sp.relayout();
    }
}

fn overset_count(doc: &Document) -> usize {
    doc.stories
        .keys()
        .filter(|sid| designcraft_compose::compose_story(doc, **sid, &designcraft_compose::ComposeOptions::default()).is_overset())
        .count()
}

fn add_placeholder(d: &mut Document, sel: &Selection, p: &Value, name: &str) -> Result<Value> {
    let (source_id, field) = {
        let source = if let Some(id) = p.get("source").and_then(Value::as_u64) {
            d.data_merge.sources.iter().find(|src| src.id == id).ok_or_else(|| bad("data.placeholder.add", format!("unknown data source {id}")))?
        } else {
            d.data_merge.sources.iter().find(|src| src.enabled).ok_or_else(|| bad("data.placeholder.add", "no enabled data source"))?
        };
        let field =
            source.fields.iter().find(|f| f.name == name).cloned().ok_or_else(|| bad("data.placeholder.add", format!("unknown field \"{name}\"")))?;
        (source.id, field)
    };
    let role_param = str_param(p, "role");
    let explicit_item = id_param(p, "item");
    let explicit_story = p.get("story").is_some();
    let use_item = if explicit_item.is_some() {
        true
    } else if explicit_story || role_param == Some("text") {
        false
    } else if matches!(role_param, Some("image" | "qr")) {
        true
    } else {
        sel.text.is_none() && !sel.items.is_empty()
    };
    let role = if let Some(s) = role_param {
        PlaceholderRole::parse(s).ok_or_else(|| bad("data.placeholder.add", format!("unknown role \"{s}\"")))?
    } else if use_item {
        match field.kind {
            DataFieldKind::Image => PlaceholderRole::Image,
            DataFieldKind::Qr => PlaceholderRole::Qr,
            DataFieldKind::Text => return Err(bad("data.placeholder.add", "say which role to use on this frame")),
        }
    } else {
        PlaceholderRole::Text
    };
    let anchor = if use_item {
        if matches!(role, PlaceholderRole::Text) {
            return Err(bad("data.placeholder.add", "text placeholders go in a story"));
        }
        let item = explicit_item.or_else(|| sel.items.first().copied()).ok_or_else(|| bad("data.placeholder.add", "select text or a frame"))?;
        if d.item(item).is_none() {
            return Err(bad("data.placeholder.add", format!("no object with id {}", item.0)));
        }
        if matches!(role, PlaceholderRole::Image | PlaceholderRole::Qr)
            && let Some(it) = d.item_mut(item)
        {
            it.label = format!("<<{name}>>");
        }
        PlaceholderAnchor::Item { id: item }
    } else {
        if matches!(role, PlaceholderRole::Image | PlaceholderRole::Qr) {
            return Err(bad("data.placeholder.add", "image and qr placeholders bind to a frame"));
        }
        let (story, at, end) = text_target(p, sel).ok_or_else(|| bad("data.placeholder.add", "select text or a frame"))?;
        if d.story(story).is_none() {
            return Err(bad("data.placeholder.add", "no story"));
        }
        if role == PlaceholderRole::Text {
            let marker = format!("<<{name}>>");
            let at = {
                let text = d.story(story).map(|st| st.text.clone()).unwrap_or_default();
                designcraft_doc::floor_char_boundary(&text, at.min(text.len()))
            };
            if let Some(st) = d.story_mut(story) {
                st.replace(at..at, &marker);
            }
            shift_after(d, story, at, marker.len() as isize);
            PlaceholderAnchor::Text { story, start: at, end: at.saturating_add(marker.len()) }
        } else {
            let text = d.story(story).map(|st| st.text.clone()).unwrap_or_default();
            let start = designcraft_doc::floor_char_boundary(&text, at.min(text.len()));
            let end = designcraft_doc::floor_char_boundary(&text, end.min(text.len())).max(start);
            PlaceholderAnchor::Text { story, start, end }
        }
    };
    let id = d.alloc();
    d.data_merge.placeholders.push(Placeholder { id, source_id, field: name.to_string(), role, anchor });
    Ok(json!({"id": id}))
}

fn text_target(p: &Value, sel: &Selection) -> Option<(StoryId, usize, usize)> {
    if let Some(n) = p.get("story").and_then(Value::as_u64) {
        let at = p.get("at").and_then(Value::as_u64).unwrap_or(0) as usize;
        let end = p.get("end").and_then(Value::as_u64).map(|v| v as usize).unwrap_or(at);
        return Some((StoryId(n), at, end.max(at)));
    }
    sel.text.map(|t| {
        let r = t.range();
        (t.story, r.start, r.end)
    })
}

fn shift_after(doc: &mut Document, story: StoryId, at: usize, delta: isize) {
    if delta == 0 {
        return;
    }
    for ph in &mut doc.data_merge.placeholders {
        if let PlaceholderAnchor::Text { story: s, start, end } = &mut ph.anchor
            && *s == story
            && *start >= at
        {
            *start = fill::add_delta(*start, delta);
            *end = fill::add_delta(*end, delta);
        }
    }
    for link in &mut doc.hyperlinks {
        if let HyperlinkSource::Text { story: s, start, end } = &mut link.source
            && *s == story
            && *start >= at
        {
            *start = fill::add_delta(*start, delta);
            *end = fill::add_delta(*end, delta);
        }
    }
}

/// After an edit, drop a text placeholder whose marker is gone unless the story still has exactly
/// one copy of it, and drop an item placeholder whose frame is gone.
pub(crate) fn sync_placeholders(doc: &mut Document) {
    if doc.data_merge.placeholders.is_empty() {
        return;
    }
    let old = std::mem::take(&mut doc.data_merge.placeholders);
    let mut keep = Vec::with_capacity(old.len());
    for mut ph in old {
        match ph.anchor.clone() {
            PlaceholderAnchor::Item { id } => {
                if doc.item(id).is_some() {
                    keep.push(ph);
                }
            }
            PlaceholderAnchor::Text { story, start, end } => {
                let text = doc.story(story).map(|st| st.text.clone()).unwrap_or_default();
                if ph.role != PlaceholderRole::Text {
                    if start <= end && end <= text.len() {
                        keep.push(ph);
                    }
                    continue;
                }
                let marker = format!("<<{}>>", ph.field);
                if text.get(start..end) == Some(marker.as_str()) {
                    keep.push(ph);
                    continue;
                }
                let found = marker_ranges(&text, &marker);
                if found.len() == 1
                    && let Some(&(s, e)) = found.first()
                {
                    ph.anchor = PlaceholderAnchor::Text { story, start: s, end: e };
                    keep.push(ph);
                }
            }
        }
    }
    doc.data_merge.placeholders = keep;
}

fn marker_ranges(text: &str, marker: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    if marker.is_empty() {
        return out;
    }
    let mut i = 0;
    while i < text.len() {
        if text[i..].starts_with(marker) {
            let end = i.saturating_add(marker.len());
            out.push((i, end));
            i = end;
            continue;
        }
        i = i.saturating_add(text[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1));
    }
    out
}

/// Refresh `relative_path` from the document file's folder.
pub(crate) fn refresh_relative_paths(doc: &mut Document, doc_path: &Path) {
    let Some(dir) = doc_path.parent().filter(|p| !p.as_os_str().is_empty()) else { return };
    for src in &mut doc.data_merge.sources {
        if let Some(path) = src.path.clone() {
            src.relative_path = Some(relative_between(dir, Path::new(&path)));
        }
    }
}

/// On open, compare the absolute path with its fingerprint. A missing absolute path is tried
/// again as `relative_path` beside the document.
pub(crate) fn resolve_sources_on_open(doc: &mut Document, doc_path: Option<&Path>) {
    for src in &mut doc.data_merge.sources {
        let abs_exists = src.path.as_deref().is_some_and(|p| Path::new(p).is_file());
        if abs_exists {
            if let Some(path) = src.path.clone() {
                src.status = status_of(Path::new(&path), src.fingerprint);
            }
            continue;
        }
        if let (Some(doc_path), Some(rel)) = (doc_path, src.relative_path.clone())
            && let Some(dir) = doc_path.parent()
        {
            let candidate = dir.join(&rel);
            if candidate.is_file() {
                let saved = candidate.to_string_lossy().into_owned();
                src.fingerprint = fingerprint_of(Path::new(&saved));
                src.status = SourceStatus::Ok;
                src.relative_path = Some(relative_between(dir, Path::new(&saved)));
                src.path = Some(saved);
                continue;
            }
        }
        if src.path.is_some() {
            src.status = SourceStatus::Missing;
        }
    }
}

fn read_source(p: &Value) -> std::result::Result<(DataSource, Value), String> {
    if p.get("rows").is_some() || p.get("csv").is_some() || p.get("json").is_some() {
        let (table, _) = load_inline(p)?;
        let name = if p.get("json").is_some() {
            str_param(p, "name").filter(|s| !s.is_empty()).unwrap_or("data.json")
        } else if p.get("csv").is_some() {
            str_param(p, "name").filter(|s| !s.is_empty()).unwrap_or("data.csv")
        } else {
            str_param(p, "name").filter(|s| !s.is_empty()).unwrap_or("rows")
        };
        return Ok(finish_source(table, name.to_string(), None, None));
    }
    let sheet = sheet_param(p);
    let delimiter = delim_param(p)?;
    let (bytes, name, path) = take_file(p)?;
    let table = parse_file(&bytes, &name, delimiter, sheet.as_deref())?;
    Ok(finish_source(table, name, path, sheet))
}

fn finish_source(table: Table, name: String, path: Option<String>, sheet: Option<String>) -> (DataSource, Value) {
    let fingerprint = path.as_deref().and_then(|path| fingerprint_of(Path::new(path)));
    let report = json!({
        "fields": field_pairs(&table.fields),
        "records": table.rows.len(),
        "warnings": table.warnings.clone(),
    });
    let source = DataSource {
        id: 0,
        path,
        relative_path: None,
        name,
        delimiter: table.delimiter,
        sheet,
        fields: table.fields,
        rows: table.rows,
        fingerprint,
        status: SourceStatus::Ok,
        warnings: table.warnings,
        enabled: true,
        filter: SourceFilter::default(),
        sort: Vec::new(),
    };
    (source, report)
}

/// If this document was produced by a merge, switch to the template it came from.
fn use_merge_template(s: &mut Session) -> Result<()> {
    let found = {
        let Some(uid) = s.active().and_then(|st| st.doc.data_merge.template_uid) else {
            return Ok(());
        };
        s.documents().iter().position(|d| d.uid == uid)
    };
    let Some(idx) = found else {
        return Err(bad("data.merge", "the template for this merged document is no longer open"));
    };
    s.set_active(idx);
    Ok(())
}

fn prepare_linked(doc: &Document) -> std::result::Result<(Vec<CellRecord>, Vec<String>, Vec<(u64, SourceStatus)>), String> {
    if doc.data_merge.sources.is_empty() {
        return Err("select a data source or pass csv or rows".into());
    }
    let mut records = Vec::new();
    let mut warnings = Vec::new();
    let mut statuses = Vec::new();
    let mut any_enabled = false;
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
        if !src.enabled {
            continue;
        }
        any_enabled = true;
        let mut order = filter_rows(&src.fields, &src.rows, &src.filter)?;
        sort_rows(&src.fields, &src.rows, &mut order, &src.sort)?;
        warnings.extend(src.warnings.iter().cloned());
        let data_dir = src.path.as_deref().and_then(parent_dir);
        for i in order {
            let cells = src.rows.get(i).cloned().unwrap_or_default();
            let number = (records.len() as u32).saturating_add(1);
            records.push(CellRecord { source_id: Some(src.id), fields: src.fields.clone(), cells, data_dir: data_dir.clone(), number });
        }
    }
    if !any_enabled {
        return Err("no enabled data source".into());
    }
    if records.is_empty() {
        return Err("no records".into());
    }
    Ok((records, warnings, statuses))
}

fn records_from_table(table: Table, data_dir: Option<PathBuf>) -> (Vec<CellRecord>, Vec<String>) {
    let warnings = table.warnings;
    let fields = table.fields;
    let records = table
        .rows
        .into_iter()
        .enumerate()
        .map(|(i, cells)| CellRecord {
            source_id: None,
            fields: fields.clone(),
            cells,
            data_dir: data_dir.clone(),
            number: (i as u32).saturating_add(1),
        })
        .collect();
    (records, warnings)
}

fn apply_statuses(doc: &mut Document, notes: &[(u64, SourceStatus)]) -> bool {
    let mut changed = false;
    for (id, status) in notes {
        let Some(source) = doc.data_merge.sources.iter_mut().find(|src| src.id == *id) else { continue };
        if source.status != *status {
            source.status = *status;
            changed = true;
        }
    }
    changed
}

fn remember_statuses(s: &mut Session, notes: &[(u64, SourceStatus)]) {
    let Some(st) = s.active_mut() else { return };
    let mut doc = (*st.doc).clone();
    if !apply_statuses(&mut doc, notes) {
        return;
    }
    st.doc = Arc::new(doc);
    st.revision = st.revision.saturating_add(1);
}

fn source_index(sources: &[DataSource], id: Option<u64>) -> std::result::Result<usize, String> {
    if let Some(id) = id {
        sources.iter().position(|src| src.id == id).ok_or_else(|| format!("unknown data source {id}"))
    } else if sources.len() == 1 {
        Ok(0)
    } else if sources.is_empty() {
        Err("select a data source".into())
    } else {
        Err("pass id when more than one data source is linked".into())
    }
}

fn parse_rules(value: Option<&Value>) -> std::result::Result<Vec<FilterRule>, String> {
    let Some(value) = value else { return Ok(Vec::new()) };
    let arr = value.as_array().ok_or_else(|| "rules must be an array".to_string())?;
    let mut rules = Vec::new();
    for rule in arr {
        let field = rule.get("field").and_then(Value::as_str).ok_or_else(|| "a rule needs a field".to_string())?.to_string();
        let op = rule.get("op").and_then(Value::as_str).ok_or_else(|| "a rule needs an operator".to_string())?.to_string();
        if !matches!(op.as_str(), "equals" | "notEquals" | "contains" | "startsWith" | "empty" | "notEmpty") {
            return Err(format!("unknown operator \"{op}\""));
        }
        let text = if matches!(op.as_str(), "empty" | "notEmpty") { String::new() } else { parse::cell_string(rule.get("value"))? };
        rules.push(FilterRule { field, op, value: text });
    }
    Ok(rules)
}

fn parse_sort(value: Option<&Value>) -> std::result::Result<Vec<SortKey>, String> {
    let Some(value) = value else { return Ok(Vec::new()) };
    let arr = value.as_array().ok_or_else(|| "fields must be an array".to_string())?;
    let mut keys = Vec::new();
    for key in arr {
        let field = key.get("field").and_then(Value::as_str).ok_or_else(|| "a sort field needs a field".to_string())?.to_string();
        let direction = key.get("direction").and_then(Value::as_str).unwrap_or("asc").to_string();
        if direction != "asc" && direction != "desc" {
            return Err(format!("unknown direction \"{direction}\""));
        }
        keys.push(SortKey { field, direction });
    }
    Ok(keys)
}

fn load_inline(p: &Value) -> std::result::Result<(Table, Option<PathBuf>), String> {
    if let Some(rows) = p.get("rows") {
        let arr = rows.as_array().ok_or_else(|| "rows must be an array".to_string())?;
        return Ok((table_from_objects(arr)?, None));
    }
    if let Some(csv) = p.get("csv") {
        let text = csv.as_str().ok_or_else(|| "csv must be a string".to_string())?;
        let name = str_param(p, "name").unwrap_or("data.csv");
        return Ok((table_from_bytes(text.as_bytes(), name, delim_param(p)?)?, None));
    }
    if p.get("json").is_some() {
        let (table, _) = inline_json(p)?;
        return Ok((table, None));
    }
    let sheet = sheet_param(p);
    let delimiter = delim_param(p)?;
    let (bytes, name, path) = take_file(p)?;
    let dir = path.as_deref().and_then(parent_dir);
    Ok((parse_file(&bytes, &name, delimiter, sheet.as_deref())?, dir))
}

fn payload_present(p: &Value) -> bool {
    p.get("rows").is_some()
        || p.get("csv").is_some()
        || p.get("json").is_some()
        || p.get("bytes").is_some()
        || p.get("base64").is_some()
        || p.get("path").is_some()
}

fn inline_json(p: &Value) -> std::result::Result<(Table, String), String> {
    let text = p.get("json").and_then(Value::as_str).ok_or_else(|| "json must be a string".to_string())?;
    let name = str_param(p, "name").filter(|s| !s.is_empty()).unwrap_or("data.json").to_string();
    Ok((table_from_json(text)?, name))
}

fn take_file(p: &Value) -> std::result::Result<(Vec<u8>, String, Option<String>), String> {
    if let Some(path) = str_param(p, "path")
        && !path.is_empty()
    {
        let bytes = read_path(path)?;
        let name = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string());
        return Ok((bytes, name, Some(path.to_string())));
    }
    let encoded = str_param(p, "bytes").or_else(|| str_param(p, "base64")).ok_or_else(|| "pass path or bytes".to_string())?;
    let name = str_param(p, "name").filter(|s| !s.is_empty()).ok_or_else(|| "missing `name`".to_string())?;
    Ok((super::file::base64_decode(encoded), name.to_string(), None))
}

fn parse_file(bytes: &[u8], name: &str, delimiter: Option<Delimiter>, sheet: Option<&str>) -> std::result::Result<Table, String> {
    let ext = extension(name);
    if ext == "xls" || ext == "xlsm" {
        return Err(format!("{ext} workbooks are not supported"));
    }
    if ext == "json" {
        return table_from_json_bytes(bytes, name);
    }
    if ext == "xlsx" {
        let grid = designcraft_textimport::xlsx_records(bytes, sheet).map_err(|e| e.to_string())?;
        return table_from_grid(grid, Delimiter::Comma);
    }
    table_from_bytes(bytes, name, delimiter)
}

fn read_path(path: &str) -> std::result::Result<Vec<u8>, String> {
    match std::fs::read(path) {
        Ok(b) => Ok(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(format!("{path}: the data file is missing")),
        Err(e) => Err(format!("{path}: {e}")),
    }
}

fn parent_dir(path: &str) -> Option<PathBuf> {
    let parent = Path::new(path).parent()?;
    if parent.as_os_str().is_empty() { None } else { Some(parent.to_path_buf()) }
}

fn delim_param(p: &Value) -> std::result::Result<Option<Delimiter>, String> {
    match str_param(p, "delimiter") {
        None => Ok(None),
        Some(s) => Delimiter::parse(s).map(Some).ok_or_else(|| format!("unknown delimiter \"{s}\"")),
    }
}

fn sheet_param(p: &Value) -> Option<String> {
    str_param(p, "sheet").filter(|s| !s.is_empty()).map(str::to_string)
}

fn field_pairs(fields: &[DataField]) -> Vec<Value> {
    fields.iter().map(|f| json!({"name": f.name, "kind": f.kind.as_str()})).collect()
}

fn apply_options(opt: &mut MergeOptions, p: &Value) -> std::result::Result<(), String> {
    if let Some(v) = str_param(p, "records") {
        opt.records = v.to_string();
    }
    if let Some(n) = p.get("one").and_then(Value::as_u64) {
        opt.one = n as u32;
    } else if p.get("one").is_some() {
        return Err("one must be a record number".into());
    }
    if let Some(v) = str_param(p, "range") {
        opt.range = v.to_string();
    }
    if let Some(v) = str_param(p, "perPage") {
        opt.per_page = v.to_string();
    }
    if let Some(v) = str_param(p, "arrange") {
        opt.arrange = v.to_string();
    }
    if let Some(raw) = p.get("insets") {
        let arr = raw.as_array().ok_or_else(|| "insets need four numbers".to_string())?;
        if arr.len() != 4 {
            return Err("insets need four numbers".into());
        }
        let mut insets = [0.0; 4];
        for (i, v) in arr.iter().enumerate() {
            insets[i] = v.as_f64().filter(|n| n.is_finite()).ok_or_else(|| "insets need four numbers".to_string())?;
        }
        opt.insets = insets;
    }
    if let Some(n) = number_param(p, "columnSpacing")? {
        opt.column_spacing = n;
    }
    if let Some(n) = number_param(p, "rowSpacing")? {
        opt.row_spacing = n;
    }
    if let Some(v) = str_param(p, "fitting") {
        opt.fitting = v.to_string();
    }
    if let Some(v) = p.get("center").and_then(Value::as_bool) {
        opt.center = v;
    }
    if let Some(v) = p.get("linkImages").and_then(Value::as_bool) {
        opt.link_images = v;
    }
    if let Some(v) = p.get("limit") {
        if v.is_null() {
            opt.limit = None;
        } else if let Some(n) = v.as_u64() {
            opt.limit = Some(n as u32);
        } else {
            return Err("limit must be a number".into());
        }
    }
    Ok(())
}

fn number_param(p: &Value, key: &str) -> std::result::Result<Option<f64>, String> {
    match p.get(key) {
        None => Ok(None),
        Some(v) => v.as_f64().filter(|n| n.is_finite()).map(Some).ok_or_else(|| format!("{key} must be a number")),
    }
}

fn validate(opt: &MergeOptions) -> std::result::Result<(), String> {
    if !matches!(opt.records.as_str(), "all" | "one" | "range") {
        return Err(format!("unknown records value \"{}\"", opt.records));
    }
    if !matches!(opt.per_page.as_str(), "single" | "multiple") {
        return Err(format!("unknown perPage value \"{}\"", opt.per_page));
    }
    if !matches!(opt.arrange.as_str(), "rows" | "columns") {
        return Err(format!("unknown arrange value \"{}\"", opt.arrange));
    }
    if !matches!(opt.fitting.as_str(), "fitProportionally" | "fillProportionally" | "fitContentToFrame" | "none") {
        return Err(format!("unknown fitting \"{}\"", opt.fitting));
    }
    if opt.insets.iter().any(|n| !n.is_finite()) || !opt.column_spacing.is_finite() || !opt.row_spacing.is_finite() {
        return Err("spacing and insets must be finite numbers".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
