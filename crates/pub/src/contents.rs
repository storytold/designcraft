// Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0
//! Publication records decoded from docs/pub-format/contents-stream.md.
use crate::{
    PubError, Result,
    read::{number, properties, record, u32_at},
};
use std::collections::HashMap;
#[derive(Default, Clone)]
pub(crate) struct PageInfo {
    pub seq: u32,
    pub shapes: Vec<u32>,
    pub applied_master: Option<u32>,
}
#[derive(Default, Clone)]
pub(crate) struct ShapeInfo {
    pub text_id: Option<u32>,
    pub chain_index: u32,
    pub valign: Option<u32>,
    pub is_table: bool,
    pub table: Option<crate::tables::TableInfo>,
}
#[derive(Default)]
pub(crate) struct Contents {
    pub width_emu: u32,
    pub height_emu: u32,
    pub pages: Vec<PageInfo>,
    pub masters: Vec<PageInfo>,
    pub shapes: HashMap<u32, ShapeInfo>,
    pub palette: Vec<[u8; 3]>,
    pub warnings: Vec<String>,
}
pub(crate) fn rgb_of(c: u32) -> [u8; 3] {
    [c as u8, (c >> 8) as u8, (c >> 16) as u8]
}
pub(crate) fn parse(b: &[u8]) -> Result<Contents> {
    if b.len() > 256 * 1024 * 1024 {
        return Err(PubError::Malformed("Contents stream exceeds limit".into()));
    }
    let trailer = properties(record(b, u32_at(b, 0x1a)? as usize)?)?;
    let directory = trailer.iter().find(|p| p.kind == 0x90).ok_or_else(|| PubError::Malformed("missing record directory".into()))?.children()?;
    let mut c = Contents::default();
    let mut pages = HashMap::new();
    let mut cell_lists = HashMap::new();
    let mut total_cell_records = 0usize;
    let mut total_grid_slots = 0usize;
    let mut order = Vec::new();
    let mut found_document = false;
    for (seq, entry) in directory.iter().enumerate() {
        if entry.kind != 0x88 {
            continue;
        }
        let fields = entry.children()?;
        let (Some(ty), Some(at)) = (number(&fields, 2), number(&fields, 4)) else { continue };
        if !matches!(ty, 1 | 0x10 | 0x20 | 0x30 | 0x43 | 0x44 | 0x5c | 0x63) {
            continue;
        }
        let p = properties(record(b, at as usize)?)?;
        let seq = seq as u32;
        match ty {
            0x44 => {
                found_document = true;
                for v in &p {
                    if v.key == 2 {
                        order = v.children()?.iter().filter_map(|e| e.number()).collect();
                    }
                    if v.key == 0x12 {
                        let size = v.children()?;
                        c.width_emu = number(&size, 1).unwrap_or(0);
                        c.height_emu = number(&size, 2).unwrap_or(0);
                    }
                }
            }
            0x43 => {
                let mut page = PageInfo { seq, applied_master: number(&p, 0x0d), ..Default::default() };
                let mut master = false;
                for v in &p {
                    if v.key == 2 {
                        page.shapes = v.children()?.iter().filter_map(|e| e.number()).collect();
                    }
                    if v.key == 0x0e && v.kind == 0xc0 {
                        master = !v.text().is_empty();
                    }
                }
                pages.insert(seq, (page, master));
            }
            0x63 => {
                let cells = crate::tables::cells(&p)?;
                total_cell_records = total_cell_records.saturating_add(cells.len());
                if total_cell_records > 100_000 {
                    return Err(PubError::Malformed("too many table cell records".into()));
                }
                cell_lists.insert(seq, cells);
            }
            0x5c => {
                for array in p.iter().filter(|v| v.kind == 0xa0) {
                    for color in array.children()? {
                        let value = if color.kind == 0x88 { number(&color.children()?, 1).unwrap_or(0) } else { 0 };
                        if c.palette.len() >= 10_000 {
                            return Err(PubError::Malformed("palette exceeds limit".into()));
                        }
                        c.palette.push(rgb_of(value));
                    }
                }
            }
            _ => {
                let table = if ty == 0x10 { crate::tables::TableInfo::parse(&p)? } else { None };
                total_grid_slots = total_grid_slots.saturating_add(table.as_ref().map_or(0, |t| t.rows.len() * t.columns.len()));
                if total_grid_slots > 100_000 {
                    return Err(PubError::Malformed("too many table grid slots".into()));
                }
                c.shapes.insert(
                    seq,
                    ShapeInfo {
                        text_id: number(&p, 0x27),
                        chain_index: number(&p, 0x28).unwrap_or(0),
                        valign: number(&p, 0x35),
                        is_table: ty == 0x10,
                        table,
                    },
                );
            }
        }
    }
    let mut linked_cells = 0usize;
    for shape in c.shapes.values_mut() {
        if let Some(table) = &mut shape.table {
            if let Some(cells) = cell_lists.get(&table.cell_record) {
                linked_cells = linked_cells.saturating_add(cells.len());
                if linked_cells > 100_000 {
                    return Err(PubError::Malformed("too many linked table cells".into()));
                }
                table.cells = cells.clone();
                table.validate()?;
            } else {
                shape.table = None;
            }
        }
    }
    if !found_document || c.width_emu == 0 || c.height_emu == 0 {
        return Err(PubError::Malformed("missing document or page size".into()));
    }
    if order.len() > 10_000 {
        return Err(PubError::Malformed("too many page references".into()));
    }
    let mut ordinary = Vec::new();
    let mut total_shapes = 0usize;
    for seq in order {
        total_shapes = total_shapes.saturating_add(pages.get(&seq).map_or(0, |(p, _)| p.shapes.len()));
        if total_shapes > 1_000_000 {
            return Err(PubError::Malformed("too many page objects".into()));
        }
        if let Some((page, true)) = pages.get(&seq) {
            c.masters.push(page.clone());
        } else {
            ordinary.push(seq);
        }
    }
    let tail = ordinary.len().saturating_sub(4);
    let placeholders = ordinary.len() > 4
        && ordinary.get(tail..).ok_or(PubError::Truncated)?.iter().filter(|s| pages.get(s).is_some_and(|(p, _)| !p.shapes.is_empty())).count() <= 1;
    for (i, seq) in ordinary.iter().enumerate() {
        if let Some((page, _)) = pages.get(seq) {
            if placeholders && i >= tail {
                if !page.shapes.is_empty() {
                    c.warnings.push("Scratch-area objects were not imported.".into());
                }
            } else {
                c.pages.push(page.clone());
            }
        }
    }
    if c.pages.is_empty()
        && placeholders
        && let Some(page) = ordinary.get(tail..).into_iter().flatten().filter_map(|s| pages.get(s)).map(|(p, _)| p).find(|p| p.shapes.is_empty())
    {
        c.pages.push(page.clone());
    }
    if c.pages.len() + c.masters.len() > 10_000 {
        return Err(PubError::Malformed("too many pages".into()));
    }
    Ok(c)
}
