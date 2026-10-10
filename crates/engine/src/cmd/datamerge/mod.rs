//! Data merge commands: one linked table, placeholders, preview, and a new merged document.
//!
//! `data.merge` used to duplicate pages inside the template. It now leaves the template and its
//! undo stack alone and appends a new document.

mod fill;
mod parse;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use designcraft_doc::{
    DataField, DataFieldKind, DataSource, Delimiter, Document, HyperlinkSource, ItemId, MergeOptions, Page, Placeholder, PlaceholderAnchor,
    PlaceholderRole, Selection, SourceStatus, SpreadRef, StoryId,
};
use designcraft_geom::{Rect, Vec2};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, id_param, ok, str_param};
use crate::{DocState, Result, Session};
use fill::{FillReport, Maps, MissingImage, copy_item, fill_row, identity_map};
use parse::{
    Table, TileInput, extension, fingerprint_of, relative_between, select_records, status_of, table_from_bytes, table_from_grid, table_from_objects,
    tile,
};

const PARENT_WARNING: &str = "A parent-page placeholder was left unfilled.";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "data.source.select",
            "Select Data Source…",
            [],
            None,
            "{path | bytes|base64, name, delimiter?: comma|tab|semicolon, sheet?} → {fields, records, warnings}",
            has_doc,
            source_select
        ),
        cmd!("data.source.update", "Update Data Source", [], None, "{} → re-read the linked file", has_doc, source_update),
        cmd!("data.source.remove", "Remove Data Source", [], None, "{} → drop the source; placeholders stay", has_doc, source_remove),
        cmd!(
            query "data.fields",
            "Data Fields",
            [],
            None,
            "{csv? | rows? | path? | bytes?, name?} → [{name, kind, uses}]; no params reads the linked source and does not attach one",
            has_doc,
            fields_cmd
        ),
        cmd!(
            "data.placeholder.add",
            "Insert Field",
            [],
            None,
            "{field, role?: text|image|qr|hyperlink, story?, at?, end?, item?} → {id}",
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
            "{csv? | rows? | path? | bytes?, name?, records?, one?, range?, perPage?, arrange?, insets?, columnSpacing?, rowSpacing?, fitting?, center?, linkImages?, limit?} → {records, pages, missingImages, oversetStories, warnings}",
            has_doc,
            merge_cmd
        ),
    ]
}

fn source_select(s: &mut Session, p: &Value) -> Result<Value> {
    let (mut source, report) = read_source(p).map_err(|e| bad("data.source.select", e))?;
    s.edit(move |d, _| {
        source.id = d.data_merge.sources.first().map(|src| src.id).unwrap_or_else(|| d.alloc());
        d.data_merge.sources.clear();
        d.data_merge.sources.push(source);
        Ok(report)
    })
}

fn source_update(s: &mut Session, _p: &Value) -> Result<Value> {
    let (path, name, delimiter, sheet) = {
        let src = s.doc()?.doc.data_merge.sources.first().ok_or_else(|| bad("data.source.update", "select a data source"))?;
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
        let src = d.data_merge.sources.first_mut().ok_or_else(|| bad("data.source.update", "select a data source"))?;
        src.fields = table.fields;
        src.rows = table.rows;
        src.warnings = table.warnings;
        src.delimiter = table.delimiter;
        src.fingerprint = fingerprint_of(Path::new(&path));
        src.status = SourceStatus::Ok;
        Ok(report)
    })
}

fn source_remove(s: &mut Session, _p: &Value) -> Result<Value> {
    s.edit(|d, _| {
        d.data_merge.sources.clear();
        ok()
    })
}

fn fields_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    if payload_present(p) {
        let (table, _) = load_inline(p).map_err(|e| bad("data.fields", e))?;
        return Ok(fields_json(&table.fields, |_| 0));
    }
    let st = s.doc()?;
    let Some(src) = st.doc.data_merge.sources.first() else {
        return Ok(json!([]));
    };
    let placeholders = st.doc.data_merge.placeholders.clone();
    Ok(fields_json(&src.fields, |name| placeholders.iter().filter(|ph| ph.field == name).count()))
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
    s.edit(|d, _| {
        apply_options(&mut d.data_merge.options, p).map_err(|e| bad("data.options", e))?;
        validate(&d.data_merge.options).map_err(|e| bad("data.options", e))?;
        serde_json::to_value(&d.data_merge.options).map_err(|e| bad("data.options", e.to_string()))
    })
}

fn preview_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc_mut()?;
    let source = st.doc.data_merge.sources.first().cloned().ok_or_else(|| bad("data.preview", "select a data source"))?;
    if source.rows.is_empty() {
        return Err(bad("data.preview", "no records"));
    }
    let max = source.rows.len() as u64;
    let record = p.get("record").and_then(Value::as_u64).unwrap_or(1).clamp(1, max);
    let base = if let Some(stash) = &st.preview_stash {
        stash.clone()
    } else {
        let current = st.doc.clone();
        st.preview_stash = Some(current.clone());
        current
    };
    let mut doc = (*base).clone();
    let options = doc.data_merge.options.clone();
    let maps = identity_map(&doc);
    let row = source.rows.get((record as usize).saturating_sub(1)).cloned().unwrap_or_default();
    let data_dir = source.path.as_deref().and_then(parent_dir);
    let doc_dir = st.path.as_deref().and_then(parent_dir);
    fill_row(&mut doc, &source.fields, &row, record as u32, &options, &maps, data_dir.as_deref(), doc_dir.as_deref());
    st.doc = Arc::new(doc);
    st.preview_record = Some(record as u32);
    st.bump_revision();
    Ok(json!({"record": record}))
}

fn merge_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    if p.get("spread").is_some() {
        return Err(bad("data.merge", "the spread parameter has been removed"));
    }
    let (table, mut warnings, data_dir, doc_dir, options, selected, src, linked_status) = {
        let st = s.doc()?;
        let (table, status_warnings, data_dir, linked_status) = if payload_present(p) {
            let (table, dir) = load_inline(p).map_err(|e| bad("data.merge", e))?;
            (table, Vec::new(), dir, None)
        } else {
            let (table, warnings, dir, status) = linked_table(&st.doc).map_err(|e| bad("data.merge", e))?;
            (table, warnings, dir, Some(status))
        };
        let mut options = st.doc.data_merge.options.clone();
        apply_options(&mut options, p).map_err(|e| bad("data.merge", e))?;
        validate(&options).map_err(|e| bad("data.merge", e))?;
        let selected =
            select_records(table.rows.len(), &options.records, options.one, &options.range, options.limit).map_err(|e| bad("data.merge", e))?;
        if options.per_page == "multiple" && (st.doc.settings.facing_pages || st.doc.page_count() != 1) {
            return Err(bad("data.merge", "multiple records on a page need one page with facing pages off"));
        }
        let doc_dir = st.path.as_deref().and_then(parent_dir);
        let src = (*st.doc).clone();
        (table, status_warnings, data_dir, doc_dir, options, selected, src, linked_status)
    };
    warnings.extend(table.warnings.iter().cloned());
    let mut merged = src.clone();
    let mut missing = Vec::new();
    if options.per_page == "multiple" {
        tile_records(&mut merged, &src, &table, &selected, &options, data_dir.as_deref(), doc_dir.as_deref(), &mut warnings, &mut missing)?;
    } else {
        copy_records(&mut merged, &src, &table, &selected, &options, data_dir.as_deref(), doc_dir.as_deref(), &mut warnings, &mut missing)?;
    }
    // Status is written only after every error return. The new document then becomes active,
    // so this template edit does not push an undo step. The fingerprint stays the last read.
    if let Some(status) = linked_status {
        remember_source_status(s, status);
        set_source_status(&mut merged, status);
    }
    merged.data_merge.placeholders.clear();
    merged.title = format!("{} merged", src.title);
    let pages = merged.page_count();
    let overset = overset_count(&merged);
    let records = selected.len();
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
    table: &Table,
    selected: &[usize],
    options: &MergeOptions,
    data_dir: Option<&Path>,
    doc_dir: Option<&Path>,
    warnings: &mut Vec<String>,
    missing: &mut Vec<MissingImage>,
) -> Result<()> {
    let mut parent = false;
    if let Some(&idx) = selected.first() {
        let maps = identity_map(dst);
        fill_at(dst, table, idx, options, &maps, data_dir, doc_dir, warnings, missing, &mut parent);
    }
    for &idx in selected.iter().skip(1) {
        let maps = append_copy(dst, src)?;
        fill_at(dst, table, idx, options, &maps, data_dir, doc_dir, warnings, missing, &mut parent);
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
    table: &Table,
    selected: &[usize],
    options: &MergeOptions,
    data_dir: Option<&Path>,
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
        count: selected.len(),
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
        let Some(&idx) = selected.get(hit.index) else { continue };
        if hit.index == 0 && hit.page == 0 && hit.col == 0 && hit.row == 0 {
            let maps = identity_map(dst);
            fill_at(dst, table, idx, options, &maps, data_dir, doc_dir, warnings, missing, &mut parent);
            continue;
        }
        let Some((dsi, _)) = dst.page_loc(hit.page) else { continue };
        let mut maps = Maps::default();
        for id in &ids {
            let Some(it) = src.item(*id) else { continue };
            copy_item(dst, src, it, SpreadRef::Doc(dsi), Vec2::new(hit.dx, hit.dy), &mut maps)?;
        }
        fill_at(dst, table, idx, options, &maps, data_dir, doc_dir, warnings, missing, &mut parent);
    }
    Ok(())
}

fn fill_at(
    doc: &mut Document,
    table: &Table,
    idx: usize,
    options: &MergeOptions,
    maps: &Maps,
    data_dir: Option<&Path>,
    doc_dir: Option<&Path>,
    warnings: &mut Vec<String>,
    missing: &mut Vec<MissingImage>,
    parent: &mut bool,
) {
    let Some(row) = table.rows.get(idx) else { return };
    let report = fill_row(doc, &table.fields, row, (idx as u32).saturating_add(1), options, maps, data_dir, doc_dir);
    absorb(report, warnings, missing, parent);
}

fn absorb(report: FillReport, warnings: &mut Vec<String>, missing: &mut Vec<MissingImage>, parent: &mut bool) {
    for w in report.warnings {
        if w == PARENT_WARNING {
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
    let Some(sp) = dst.spreads.get_mut(si) else { return };
    let sp = Arc::make_mut(sp);
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
        let source = d.data_merge.sources.first().ok_or_else(|| bad("data.placeholder.add", "select a data source"))?;
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
    let sheet = sheet_param(p);
    let delimiter = delim_param(p)?;
    let (bytes, name, path) = take_file(p)?;
    let table = parse_file(&bytes, &name, delimiter, sheet.as_deref())?;
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
    };
    Ok((source, report))
}

fn linked_table(doc: &Document) -> std::result::Result<(Table, Vec<String>, Option<PathBuf>, SourceStatus), String> {
    let src = doc.data_merge.sources.first().ok_or_else(|| "select a data source or pass csv or rows".to_string())?;
    let mut warnings = Vec::new();
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
    let data_dir = src.path.as_deref().and_then(parent_dir);
    let table = Table { fields: src.fields.clone(), rows: src.rows.clone(), warnings: src.warnings.clone(), delimiter: src.delimiter };
    Ok((table, warnings, data_dir, status))
}

fn set_source_status(doc: &mut Document, status: SourceStatus) -> bool {
    let Some(source) = doc.data_merge.sources.first_mut() else {
        return false;
    };
    if source.status == status {
        return false;
    }
    source.status = status;
    true
}

fn remember_source_status(s: &mut Session, status: SourceStatus) {
    let Some(st) = s.active_mut() else { return };
    let mut doc = (*st.doc).clone();
    if !set_source_status(&mut doc, status) {
        return;
    }
    st.doc = Arc::new(doc);
    st.bump_revision();
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
    let sheet = sheet_param(p);
    let delimiter = delim_param(p)?;
    let (bytes, name, path) = take_file(p)?;
    let dir = path.as_deref().and_then(parent_dir);
    Ok((parse_file(&bytes, &name, delimiter, sheet.as_deref())?, dir))
}

fn payload_present(p: &Value) -> bool {
    p.get("rows").is_some() || p.get("csv").is_some() || p.get("bytes").is_some() || p.get("base64").is_some() || p.get("path").is_some()
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
    if ext == "xlsx" {
        let grid = designcraft_textimport::xlsx_records(bytes, sheet).map_err(|e| e.to_string())?;
        return table_from_grid(grid, Delimiter::Comma);
    }
    table_from_bytes(bytes, name, delimiter)
}

/// Largest data source file read (bytes); bigger files are refused before reading.
pub(crate) const MAX_DATA_FILE_BYTES: u64 = 256 * 1024 * 1024;
/// Largest image file a merge places (bytes); bigger candidates are skipped before reading.
pub(crate) const MAX_IMAGE_FILE_BYTES: u64 = 256 * 1024 * 1024;

fn read_path(path: &str) -> std::result::Result<Vec<u8>, String> {
    read_path_capped(path, MAX_DATA_FILE_BYTES)
}

fn read_path_capped(path: &str, cap: u64) -> std::result::Result<Vec<u8>, String> {
    match std::fs::metadata(path) {
        Ok(m) if m.len() > cap => return Err(format!("{path}: the data file is larger than {} MiB", cap / (1024 * 1024))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(format!("{path}: the data file is missing")),
        _ => {}
    }
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

fn fields_json(fields: &[DataField], uses: impl Fn(&str) -> usize) -> Value {
    Value::Array(fields.iter().map(|f| json!({"name": f.name, "kind": f.kind.as_str(), "uses": uses(&f.name)})).collect())
}

fn apply_options(opt: &mut MergeOptions, p: &Value) -> std::result::Result<(), String> {
    if let Some(v) = str_param(p, "records") {
        opt.records = v.to_string();
    }
    if let Some(n) = p.get("one").and_then(Value::as_u64) {
        opt.one = u32::try_from(n).map_err(|_| "one is too large".to_string())?;
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
            opt.limit = Some(u32::try_from(n).map_err(|_| "limit is too large".to_string())?);
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
