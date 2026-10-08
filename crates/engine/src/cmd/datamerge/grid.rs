//! Drawn data-merge grids: geometry, the shared record cursor, and the commands that edit a grid.
//!
//! A grid is a rectangle whose children are the prototype cell. Merge copies those children once
//! per cell and deletes the grid, so the result is ordinary items.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use designcraft_doc::{Content, DataGrid, Document, Item, ItemId, Selection, Shape, SpreadRef, Stroke};
use designcraft_geom::{Affine, Rect};
use serde_json::{Value, json};

use super::super::object::RemoveKeep;
use super::super::{bad, rect_param, spread_param};
use super::fill::{self, FillMode, Maps, MissingImage, place_from_snapshot, snapshot_of};
use crate::{Result, Session};

/// String errors from geometry and planning. Command handlers turn these into `EngineError`.
type GridOut<T> = std::result::Result<T, String>;

/// One record already chosen for this merge or preview.
#[derive(Clone, Debug)]
pub struct CellRecord {
    pub source_id: Option<u64>,
    pub fields: Vec<designcraft_doc::DataField>,
    pub cells: Vec<String>,
    pub data_dir: Option<std::path::PathBuf>,
    /// 1-based position in the filtered list, before range and limit.
    pub number: u32,
}

#[derive(Clone, Debug)]
pub struct GridSpec {
    pub id: ItemId,
    pub rows: u32,
    pub columns: u32,
    pub offset: u32,
    pub advance: u32,
}

#[derive(Clone, Debug)]
pub struct Visit {
    pub id: ItemId,
    pub cells: Vec<Option<usize>>,
}

pub fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let rect = rect_param(p, "rect").ok_or_else(|| bad("data.grid.create", "missing rect"))?;
    let grid = grid_from_value(p, DataGrid::default()).map_err(|e| bad("data.grid.create", e))?;
    validate_grid(&grid, rect).map_err(|e| bad("data.grid.create", e))?;
    let sr = spread_param(p, "spread");
    let lid = s.doc()?.active_layer;
    s.edit(move |d, sel| {
        // A grid is the layout. Multiple records per page is the other layout.
        d.data_merge.options.per_page = "single".to_string();
        if d.spread(sr).is_none() {
            return Err(bad("data.grid.create", "no such spread"));
        }
        if !rect_on_document_page(d, sr, rect) {
            return Err(bad("data.grid.create", "a grid must sit on a document page"));
        }
        let id = ItemId(d.alloc());
        let mut it = Item::new(id, lid, Shape::Rectangle, designcraft_geom::shapes::rectangle(rect));
        it.stroke = Stroke::default();
        it.content = Content::Group { items: Vec::new() };
        it.data_grid = Some(grid.clone());
        d.insert_item(sr, it, None)?;
        let origin = cell_rect(rect, &grid, 0).map_err(|e| bad("data.grid.create", e))?;
        let cell = d.item(id).map(|item| item.xf.transform_rect_bbox(origin)).unwrap_or(origin);
        let ids: Vec<ItemId> = d
            .spread(sr)
            .map(|sp| {
                sp.items
                    .iter()
                    .filter(|item| item.id != id && item.data_grid.is_none() && contains_rect(cell, item.bounds()))
                    .map(|item| item.id)
                    .collect()
            })
            .unwrap_or_default();
        adopt_ids(d, id, &ids, false).map_err(|e| bad("data.grid.create", e))?;
        *sel = Selection::items(vec![id]);
        Ok(json!({"id": id.0}))
    })
}

pub fn set(s: &mut Session, p: &Value) -> Result<Value> {
    let requested = p.get("id").and_then(Value::as_u64);
    s.edit(move |d, _| {
        let id = resolve_grid(d, requested).map_err(|e| bad("data.grid.set", e))?;
        let current = d.item(id).and_then(|it| it.data_grid.clone()).ok_or_else(|| bad("data.grid.set", "not a grid"))?;
        let bounds = d.item(id).map(Item::inner_bounds).unwrap_or(Rect::ZERO);
        let next = grid_from_value(p, current).map_err(|e| bad("data.grid.set", e))?;
        validate_grid(&next, bounds).map_err(|e| bad("data.grid.set", e))?;
        if let Some(it) = d.item_mut(id) {
            it.data_grid = Some(next);
        }
        crate::cmd::ok()
    })
}

pub fn adopt(s: &mut Session, p: &Value) -> Result<Value> {
    let requested = p.get("id").and_then(Value::as_u64);
    s.edit(move |d, sel| {
        let id = resolve_grid(d, requested).map_err(|e| bad("data.grid.adopt", e))?;
        let ids = sel.items.clone();
        adopt_ids(d, id, &ids, true).map_err(|e| bad("data.grid.adopt", e))?;
        *sel = Selection::items(vec![id]);
        crate::cmd::ok()
    })
}

pub fn release(s: &mut Session, p: &Value) -> Result<Value> {
    let requested = p.get("id").and_then(Value::as_u64);
    s.edit(move |d, sel| {
        let id = resolve_grid(d, requested).map_err(|e| bad("data.grid.release", e))?;
        let ids = release_one(d, id).map_err(|e| bad("data.grid.release", e))?;
        *sel = Selection::items(ids);
        crate::cmd::ok()
    })
}

/// Put every grid's children back on the page and delete the grids.
/// The count is how many grids were removed.
pub fn release_all(doc: &mut Document) -> GridOut<(usize, Vec<ItemId>)> {
    let ids = placed_grid_ids(doc)?;
    let removed = ids.len();
    let mut children = Vec::new();
    for id in ids.into_iter().rev() {
        children.extend(release_one(doc, id)?);
    }
    Ok((removed, children))
}

fn release_one(doc: &mut Document, id: ItemId) -> GridOut<Vec<ItemId>> {
    let loc = doc.find(id).ok_or_else(|| "unknown grid".to_string())?;
    if loc.path.len() != 1 {
        return Err("a grid must sit on a document page".into());
    }
    let spread = loc.spread;
    let at = loc.path[0];
    let grid = doc.remove_item_keep_story(id).map_err(|e| e.to_string())?;
    let Some(spec) = grid.data_grid.clone() else {
        return Err("not a grid".into());
    };
    let origin = cell_rect(grid.inner_bounds(), &spec, 0)?;
    let mut ids = Vec::new();
    let children = match grid.content {
        Content::Group { items } => items,
        _ => Vec::new(),
    };
    for (k, child) in children.into_iter().enumerate() {
        let mut child = Arc::unwrap_or_clone(child);
        child.xf = grid.xf * Affine::translate((origin.x0, origin.y0)) * child.xf;
        ids.push(child.id);
        doc.insert_item(spread, child, Some(at.saturating_add(k))).map_err(|e| e.to_string())?;
    }
    Ok(ids)
}

/// Grids in visit order. An empty list means phase 1 merge. Placement and cell size are checked.
/// Multiple records per page is the other layout, so the two cannot be combined.
pub fn grids_for_merge(doc: &Document, per_page: &str) -> GridOut<Vec<GridSpec>> {
    let ids = placed_grid_ids(doc)?;
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    if per_page == "multiple" {
        return Err("multiple records on a page cannot be combined with a grid".into());
    }
    let mut specs = Vec::new();
    for id in ids {
        let it = doc.item(id).ok_or_else(|| "a grid must sit on a document page".to_string())?;
        let grid = it.data_grid.clone().ok_or_else(|| "a grid must sit on a document page".to_string())?;
        validate_grid(&grid, it.inner_bounds())?;
        specs.push(GridSpec { id, rows: grid.rows, columns: grid.columns, offset: grid.record_offset, advance: grid.record_advance });
    }
    Ok(specs)
}

/// Cycles of visits. The cursor walks `len` records. Offset applies only on the first visit.
pub fn plan_cycles(len: usize, grids: &[GridSpec]) -> GridOut<Vec<Vec<Visit>>> {
    if grids.is_empty() {
        return Ok(Vec::new());
    }
    let mut cursor = 0usize;
    let mut first_visit = vec![true; grids.len()];
    let mut cycles = Vec::new();
    loop {
        let mut probe = cursor;
        if first_visit.first().copied().unwrap_or(false) {
            probe = probe.saturating_add(grids.first().map(|g| g.offset as usize).unwrap_or(0));
        }
        if probe >= len {
            break;
        }
        let mut visits = Vec::with_capacity(grids.len());
        for (gi, g) in grids.iter().enumerate() {
            if first_visit.get(gi).copied().unwrap_or(false) {
                cursor = cursor.saturating_add(g.offset as usize);
                if let Some(flag) = first_visit.get_mut(gi) {
                    *flag = false;
                }
            }
            visits.push(Visit { id: g.id, cells: take_cells(&mut cursor, g, len) });
        }
        cycles.push(visits);
        if cycles.len() > len.saturating_add(1) {
            break;
        }
    }
    if cycles.is_empty() {
        return Err("the first grid's record offset leaves its origin cell empty".into());
    }
    Ok(cycles)
}

/// One pass starting at `start`, with no offsets. Cells past `len` are blank.
pub fn plan_preview(start: usize, grids: &[GridSpec], len: usize) -> Vec<Visit> {
    let mut cursor = start;
    grids.iter().map(|g| Visit { id: g.id, cells: take_cells(&mut cursor, g, len) }).collect()
}

fn take_cells(cursor: &mut usize, grid: &GridSpec, len: usize) -> Vec<Option<usize>> {
    let n = (grid.rows as usize).saturating_mul(grid.columns as usize).max(1);
    let mut cells = Vec::with_capacity(n);
    if grid.advance == 0 {
        let rec = (*cursor < len).then_some(*cursor);
        for _ in 0..n {
            cells.push(rec);
        }
        *cursor = cursor.saturating_add(1);
    } else {
        let step = grid.advance as usize;
        for _ in 0..n {
            cells.push((*cursor < len).then_some(*cursor));
            *cursor = cursor.saturating_add(step);
        }
    }
    cells
}

/// Fill loose items from `loose_record` and repeat each grid's children across its cells.
pub fn explode_cycle(
    dst: &mut Document,
    maps: &Maps,
    records: &[CellRecord],
    visits: &[Visit],
    loose_record: Option<usize>,
    options: &designcraft_doc::MergeOptions,
    doc_dir: Option<&std::path::Path>,
    warnings: &mut Vec<String>,
    missing: &mut Vec<MissingImage>,
    parent_warned: &mut bool,
) -> Result<()> {
    let grid_ids: Vec<ItemId> = visits.iter().filter_map(|visit| maps.items.get(&visit.id).copied()).collect();
    let loose = loose_maps(maps, &grid_ids, dst);
    if let Some(idx) = loose_record
        && let Some(rec) = records.get(idx)
    {
        absorb(fill_record(dst, rec, options, &loose, doc_dir), warnings, missing, parent_warned);
    }
    for visit in visits {
        let Some(&gid) = maps.items.get(&visit.id) else { continue };
        let Some(grid) = dst.item(gid).cloned() else { continue };
        let Some(loc) = dst.find(gid) else { continue };
        let Some(spec) = grid.data_grid.clone() else { continue };
        let kids: Vec<Item> = grid.children().iter().map(|child| (**child).clone()).collect();
        let snap = snapshot_of(dst, &kids);
        let bounds = grid.inner_bounds();
        let xf = grid.xf;
        let spread = loc.spread;
        for (i, cell) in visit.cells.iter().enumerate() {
            let rect = cell_rect(bounds, &spec, i).map_err(|e| bad("data.merge", e))?;
            let mut cell_maps = Maps::default();
            for child in &kids {
                let placed = xf * Affine::translate((rect.x0, rect.y0)) * child.xf;
                place_from_snapshot(dst, &snap, child, spread, placed, &mut cell_maps)?;
            }
            if let Some(idx) = *cell
                && let Some(rec) = records.get(idx)
            {
                absorb(fill_record(dst, rec, options, &cell_maps, doc_dir), warnings, missing, parent_warned);
            } else {
                let report = fill::fill_row(dst, &[], &[], 0, options, &cell_maps, None, doc_dir, FillMode::Blank);
                absorb(report, warnings, missing, parent_warned);
            }
        }
        dst.remove_item(gid)?;
    }
    Ok(())
}

fn fill_record(
    doc: &mut Document,
    rec: &CellRecord,
    options: &designcraft_doc::MergeOptions,
    maps: &Maps,
    doc_dir: Option<&std::path::Path>,
) -> fill::FillReport {
    let mode = match rec.source_id {
        Some(id) => FillMode::Source(id),
        None => FillMode::Named,
    };
    fill::fill_row(doc, &rec.fields, &rec.cells, rec.number, options, maps, rec.data_dir.as_deref(), doc_dir, mode)
}

fn absorb(report: fill::FillReport, warnings: &mut Vec<String>, missing: &mut Vec<MissingImage>, parent: &mut bool) {
    for warning in report.warnings {
        if warning == fill::PARENT_WARNING {
            if *parent {
                continue;
            }
            *parent = true;
        }
        warnings.push(warning);
    }
    missing.extend(report.missing);
}

fn loose_maps(full: &Maps, grid_ids: &[ItemId], doc: &Document) -> Maps {
    let mut grid_items = HashSet::new();
    for id in grid_ids {
        if let Some(it) = doc.item(*id) {
            it.walk(&mut |item| {
                grid_items.insert(item.id);
            });
        }
    }
    let items: HashMap<ItemId, ItemId> =
        full.items.iter().filter(|(from, to)| !grid_items.contains(from) && !grid_items.contains(to)).map(|(from, to)| (*from, *to)).collect();
    let mut stories = HashMap::new();
    for to in items.values() {
        let Some(it) = doc.item(*to) else { continue };
        let Content::Text(frame) = &it.content else { continue };
        if let Some((src, dest)) = full.stories.iter().find(|(_, dest)| **dest == frame.story) {
            stories.insert(*src, *dest);
        } else {
            stories.insert(frame.story, frame.story);
        }
    }
    Maps { items, stories }
}

fn placed_grid_ids(doc: &Document) -> GridOut<Vec<ItemId>> {
    let mut found: Vec<(usize, usize, ItemId)> = Vec::new();
    for sp in &doc.spreads {
        for (idx, it) in sp.items.iter().enumerate() {
            let mut nested = false;
            it.walk(&mut |item| {
                if item.id != it.id && item.data_grid.is_some() {
                    nested = true;
                }
            });
            if nested {
                return Err("a grid must sit on a document page".into());
            }
            if it.data_grid.is_some() {
                let page = doc.page_of_item(it.id).ok_or_else(|| "a grid must sit on a document page".to_string())?;
                found.push((page, idx, it.id));
            }
        }
    }
    for sp in &doc.parents {
        for it in &sp.items {
            let mut bad_parent = false;
            it.walk(&mut |item| {
                if item.data_grid.is_some() {
                    bad_parent = true;
                }
            });
            if bad_parent {
                return Err("a grid must sit on a document page".into());
            }
        }
    }
    found.sort_by_key(|(page, idx, _)| (*page, *idx));
    Ok(found.into_iter().map(|(_, _, id)| id).collect())
}

fn resolve_grid(doc: &Document, requested: Option<u64>) -> GridOut<ItemId> {
    let ids = placed_grid_ids(doc)?;
    if let Some(id) = requested {
        let id = ItemId(id);
        if ids.contains(&id) { Ok(id) } else { Err(format!("unknown grid {}", id.0)) }
    } else if ids.len() == 1 {
        ids.first().copied().ok_or_else(|| "unknown grid".to_string())
    } else if ids.is_empty() {
        Err("there is no grid".into())
    } else {
        Err("pass id when more than one grid is in the document".into())
    }
}

fn adopt_ids(doc: &mut Document, grid_id: ItemId, ids: &[ItemId], strict_page: bool) -> GridOut<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let grid_page = doc.page_of_item(grid_id);
    let grid_spread = doc.find(grid_id).map(|loc| loc.spread);
    for id in ids {
        if *id == grid_id {
            return Err("a grid cannot adopt itself".into());
        }
        let Some(it) = doc.item(*id) else {
            return Err(format!("no object with id {}", id.0));
        };
        if it.data_grid.is_some() {
            return Err("a grid cannot adopt another grid".into());
        }
        let Some(loc) = doc.find(*id) else {
            return Err(format!("no object with id {}", id.0));
        };
        if loc.path.len() != 1 {
            return Err("a grid cannot adopt a nested object".into());
        }
        if strict_page {
            let page = doc.page_of_item(*id);
            if page.is_none() || page != grid_page || doc.find(*id).map(|loc| loc.spread) != grid_spread {
                return Err("an item on another page cannot be adopted".into());
            }
        }
    }
    let grid = doc.item(grid_id).cloned().ok_or_else(|| "unknown grid".to_string())?;
    let spec = grid.data_grid.clone().ok_or_else(|| "not a grid".to_string())?;
    let origin = cell_rect(grid.inner_bounds(), &spec, 0)?;
    let det = grid.xf.determinant();
    if !det.is_finite() || det.abs() < 1e-12 {
        return Err("the grid transform cannot be inverted".into());
    }
    let inv = grid.xf.inverse();
    let mut kids = Vec::with_capacity(ids.len());
    for id in ids {
        let mut item = doc.remove_item_keep_story(*id).map_err(|e| e.to_string())?;
        item.xf = Affine::translate((-origin.x0, -origin.y0)) * inv * item.xf;
        kids.push(Arc::new(item));
    }
    let Some(grid) = doc.item_mut(grid_id) else {
        return Err("unknown grid".into());
    };
    match &mut grid.content {
        Content::Group { items } => items.extend(kids),
        other => *other = Content::Group { items: kids },
    }
    Ok(())
}

fn grid_from_value(p: &Value, mut grid: DataGrid) -> GridOut<DataGrid> {
    if let Some(n) = u32_field(p, "rows")? {
        grid.rows = n;
    }
    if let Some(n) = u32_field(p, "columns")? {
        grid.columns = n;
    }
    if let Some(n) = f64_field(p, "gutter")? {
        grid.gutter = n;
    }
    if let Some(n) = u32_field(p, "recordOffset")? {
        grid.record_offset = n;
    }
    if let Some(n) = u32_field(p, "recordAdvance")? {
        grid.record_advance = n;
    }
    if let Some(value) = p.get("origin").and_then(Value::as_str) {
        grid.origin = value.to_string();
    }
    if let Some(value) = p.get("arrange").and_then(Value::as_str) {
        grid.arrange = value.to_string();
    }
    Ok(grid)
}

fn u32_field(p: &Value, key: &str) -> GridOut<Option<u32>> {
    match p.get(key) {
        None => Ok(None),
        Some(value) => {
            let n = value.as_u64().ok_or_else(|| format!("{key} must be a whole number"))?;
            u32::try_from(n).map(Some).map_err(|_| format!("{key} must be a whole number"))
        }
    }
}

fn f64_field(p: &Value, key: &str) -> GridOut<Option<f64>> {
    match p.get(key) {
        None => Ok(None),
        Some(value) => value.as_f64().filter(|n| n.is_finite()).map(Some).ok_or_else(|| format!("{key} must be a number")),
    }
}

pub fn validate_grid(grid: &DataGrid, bounds: Rect) -> GridOut<()> {
    if grid.rows < 1 || grid.columns < 1 {
        return Err("rows and columns must be at least 1".into());
    }
    if grid.rows.checked_mul(grid.columns).is_none_or(|n| n > 500) {
        return Err("a grid has at most 500 cells".into());
    }
    if !grid.gutter.is_finite() || grid.gutter < 0.0 {
        return Err("gutter must be a finite number that is at least 0".into());
    }
    if !matches!(grid.origin.as_str(), "topLeft" | "topRight" | "bottomLeft" | "bottomRight") {
        return Err(format!("unknown origin \"{}\"", grid.origin));
    }
    if !matches!(grid.arrange.as_str(), "rows" | "columns") {
        return Err(format!("unknown arrange value \"{}\"", grid.arrange));
    }
    let _ = cell_rect(bounds, grid, 0)?;
    Ok(())
}

fn cell_rect(bounds: Rect, grid: &DataGrid, index: usize) -> GridOut<Rect> {
    grid.cell_rect(bounds, index).ok_or_else(|| {
        if !matches!(grid.origin.as_str(), "topLeft" | "topRight" | "bottomLeft" | "bottomRight") {
            format!("unknown origin \"{}\"", grid.origin)
        } else if grid.arrange != "rows" && grid.arrange != "columns" {
            format!("unknown arrange value \"{}\"", grid.arrange)
        } else {
            "a grid cell must have a positive width and height".to_string()
        }
    })
}

/// The rectangle's center lies inside a page of a document spread.
fn rect_on_document_page(doc: &Document, sr: SpreadRef, rect: Rect) -> bool {
    let SpreadRef::Doc(_) = sr else { return false };
    let Some(sp) = doc.spread(sr) else { return false };
    let c = rect.center();
    if !c.x.is_finite() || !c.y.is_finite() {
        return false;
    }
    sp.pages.iter().any(|page| {
        let b = page.bounds();
        c.x >= b.x0 && c.x < b.x1 && c.y >= b.y0 && c.y < b.y1
    })
}

fn contains_rect(outer: Rect, inner: Rect) -> bool {
    const EPS: f64 = 0.05;
    inner.width() >= 0.0
        && inner.height() >= 0.0
        && inner.x0 >= outer.x0 - EPS
        && inner.y0 >= outer.y0 - EPS
        && inner.x1 <= outer.x1 + EPS
        && inner.y1 <= outer.y1 + EPS
}
