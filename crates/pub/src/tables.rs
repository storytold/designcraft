// Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0
//! Confirmed table geometry and cell-text boundaries; see docs/pub-format/tables.md.
use crate::{
    PubError, Result,
    read::{Prop, count, number, slice, u32_at},
};
use std::ops::Range;

const MAX_CELLS: usize = 10_000;
fn bad(message: &str) -> PubError {
    PubError::Malformed(format!("table: {message}"))
}
#[derive(Clone, Debug)]
pub(crate) struct CellInfo {
    pub row: usize,
    pub last_row: usize,
    pub col: usize,
    pub last_col: usize,
}
#[derive(Clone)]
pub(crate) struct TableInfo {
    /// Native heights derived from consecutive stored row endings.
    pub rows: Vec<u32>,
    pub columns: Vec<u32>,
    pub cells: Vec<CellInfo>,
    pub cell_record: u32,
    /// Largest absolute ending-versus-size-sum difference in this table, for diagnostics only.
    pub row_ending_difference: u32,
}
impl TableInfo {
    pub(crate) fn parse(p: &[Prop<'_>]) -> Result<Option<Self>> {
        let (Some(rows), Some(cols), Some(cell_record)) = (number(p, 0x66), number(p, 0x67), number(p, 0x6b)) else { return Ok(None) };
        let rows = count(rows, 256)?;
        let cols = count(cols, 256)?;
        if rows == 0 || cols == 0 || rows * cols > MAX_CELLS {
            return Err(bad("grid exceeds limits"));
        }
        let Some(dimensions) = p.iter().find(|p| p.key == 0x6d && p.kind == 0x90) else { return Ok(None) };
        let entries = dimensions.children()?;
        if entries.len() != rows + cols {
            return Err(bad("dimension count mismatch"));
        }
        let mut sizes = Vec::new();
        let mut sum = 0u32;
        let mut previous_ending = 0u32;
        let mut row_ending_difference = 0u32;
        for (i, entry) in entries.iter().enumerate() {
            if i == cols {
                sum = 0;
            }
            let fields = entry.children()?;
            let size = number(&fields, 2).ok_or_else(|| bad("missing dimension"))?;
            if size == 0 || size > 197_510_400 {
                return Err(bad("dimension out of range"));
            }
            sum = sum.checked_add(size).ok_or_else(|| bad("dimension overflow"))?;
            let ending = number(&fields, 1).ok_or_else(|| bad("missing cumulative dimension"))?;
            let layout_size = if i < cols {
                if ending != sum {
                    return Err(bad("inconsistent cumulative dimension"));
                }
                size
            } else {
                if ending > i32::MAX as u32 || ending <= previous_ending {
                    return Err(bad("row endings must be positive and strictly increasing"));
                }
                let height = ending.checked_sub(previous_ending).ok_or_else(|| bad("row dimension overflow"))?;
                if height > 197_510_400 {
                    return Err(bad("row dimension out of range"));
                }
                previous_ending = ending;
                row_ending_difference = row_ending_difference.max(ending.abs_diff(sum));
                height
            };
            if i + 1 == cols && number(p, 0x68) != Some(sum) || i + 1 == cols + rows && number(p, 0x69) != Some(sum) {
                return Err(bad("size total mismatch"));
            }
            sizes.push(layout_size);
        }
        Ok(Some(Self {
            columns: sizes.get(..cols).ok_or_else(|| bad("missing columns"))?.to_vec(),
            rows: sizes.get(cols..).ok_or_else(|| bad("missing rows"))?.to_vec(),
            cells: Vec::new(),
            cell_record,
            row_ending_difference,
        }))
    }
    pub(crate) fn validate(&self) -> Result<()> {
        let mut occupied = vec![false; self.rows.len() * self.columns.len()];
        for c in &self.cells {
            if c.row > c.last_row || c.col > c.last_col || c.last_row >= self.rows.len() || c.last_col >= self.columns.len() {
                return Err(bad("invalid cell rectangle"));
            }
            for r in c.row..=c.last_row {
                for col in c.col..=c.last_col {
                    let slot = occupied.get_mut(r * self.columns.len() + col).ok_or_else(|| bad("cell out of range"))?;
                    if *slot {
                        return Err(bad("overlapping cells"));
                    }
                    *slot = true;
                }
            }
        }
        if occupied.iter().any(|v| !v) {
            return Err(bad("incomplete cell grid"));
        }
        Ok(())
    }
}
pub(crate) fn cells(p: &[Prop<'_>]) -> Result<Vec<CellInfo>> {
    let n = count(number(p, 1).ok_or_else(|| bad("missing cell count"))?, MAX_CELLS)?;
    let Some(array) = p.iter().find(|p| p.key == 2 && p.kind == 0xa0) else { return Err(bad("missing cell list")) };
    let entries = array.children()?;
    if entries.len() != n {
        return Err(bad("cell count mismatch"));
    }
    entries
        .iter()
        .map(|entry| {
            let p = entry.children()?;
            Ok(CellInfo {
                row: number(&p, 1).unwrap_or(0) as usize,
                last_row: number(&p, 2).unwrap_or(0) as usize,
                col: number(&p, 3).unwrap_or(0) as usize,
                last_col: number(&p, 4).unwrap_or(0) as usize,
            })
        })
        .collect()
}
pub(crate) fn text_ranges(data: &[u8], units: &[u16]) -> Result<Vec<Range<usize>>> {
    let n = count(u32_at(data, 0)?, MAX_CELLS - 1)? + 1;
    slice(data, 12, n * 4)?;
    let mut ranges = Vec::new();
    let mut start = 0;
    for i in 0..n {
        let end = u32_at(data, 12 + 4 * i)? as usize;
        if end < start || end > units.len() {
            return Err(bad("invalid cell text boundary"));
        }
        let last = i + 1 == n;
        if last && end != units.len() || !last && units.get(end) != Some(&13) {
            return Err(bad("cell text separator mismatch"));
        }
        let z = if last && end > start && units.get(end - 1) == Some(&13) { end - 1 } else { end };
        ranges.push(start..z);
        start = end + usize::from(!last);
    }
    Ok(ranges)
}

/// Recover only the verified one-table/one-TCD case in a multi-story publication.
/// The Contents link selects the table story; the documented TCD boundaries must
/// independently match only that story, and the cell records must be row-major.
/// Do not assign a meaning to the directory occurrence field or infer a story by order.
pub(crate) fn recover_single_table_text(c: &crate::contents::Contents, q: &mut crate::quill::Quill, bytes: &[u8]) -> Result<()> {
    if q.stories.len() <= 1 {
        return Ok(());
    }
    let mut tables = c.shapes.values().filter(|s| s.is_table);
    let Some(shape) = tables.next() else { return Ok(()) };
    if tables.next().is_some() {
        return Ok(());
    }
    let (Some(id), Some(info)) = (shape.text_id, shape.table.as_ref()) else { return Ok(()) };
    if info.cells.is_empty()
        || info.cells.len() != info.rows.len() * info.columns.len()
        || !info.cells.iter().enumerate().all(|(i, cell)| {
            cell.row == i / info.columns.len() && cell.col == i % info.columns.len() && cell.last_row == cell.row && cell.last_col == cell.col
        })
    {
        return Ok(());
    }
    let Some(data) = single_tcd(bytes)? else { return Ok(()) };
    if data.len() != 12 + 4 * info.cells.len()
        || u32_at(data, 4)? != 0
        || u32_at(data, 8)? != 0xff00
        || usize::try_from(u32_at(data, 0)?).ok().and_then(|n| n.checked_add(1)) != Some(info.cells.len())
    {
        return Ok(());
    }
    let Some(last) = info.cells.len().checked_sub(1).and_then(|i| i.checked_mul(4)).and_then(|n| n.checked_add(12)) else { return Ok(()) };
    let Ok(end) = u32_at(data, last) else { return Ok(()) };
    let mut matched = None;
    let mut work = 0usize;
    for (&story_id, story) in &q.stories {
        if story.units.len() != end as usize || story.units.last() != Some(&13) {
            continue;
        }
        work = work.saturating_add(info.cells.len());
        if work > 1_000_000 {
            return Ok(());
        }
        if let Ok(ranges) = text_ranges(data, &story.units) {
            if matched.is_some() {
                return Ok(());
            }
            matched = Some((story_id, ranges));
        }
    }
    if let Some((story_id, ranges)) = matched
        && story_id == id
        && let Some(story) = q.stories.get_mut(&id)
    {
        story.cell_ranges = Some(ranges);
        q.warnings.retain(|w| w != "Publisher table text across multiple stories/TCD sections is not decoded.");
    }
    Ok(())
}

fn single_tcd(bytes: &[u8]) -> Result<Option<&[u8]>> {
    use crate::read::u16_at;
    let mut seen = std::collections::HashSet::new();
    let mut at = 0x18usize;
    let mut found = None;
    while at != u32::MAX as usize {
        if seen.len() >= 64 || !seen.insert(at) {
            return Err(bad("Quill section directory cycle or depth limit"));
        }
        let header = slice(bytes, at, 8)?;
        let n = usize::from(u16_at(header, 2)?);
        let start = at.checked_add(8).ok_or_else(|| bad("section directory overflow"))?;
        for entry in slice(bytes, start, n * 24)?.chunks_exact(24) {
            if entry.get(2..6) == Some(b"TCD ") {
                if found.is_some() {
                    return Ok(None);
                }
                found = Some(slice(bytes, u32_at(entry, 16)? as usize, u32_at(entry, 20)? as usize)?);
            }
        }
        at = u32_at(header, 4)? as usize;
    }
    Ok(found)
}

#[cfg(test)]
mod record_tests {
    use super::*;
    use crate::read::properties;
    fn grid() -> TableInfo {
        TableInfo {
            rows: vec![457_200; 2],
            columns: vec![1_828_800; 2],
            cell_record: 260,
            row_ending_difference: 0,
            cells: vec![
                CellInfo { row: 0, last_row: 0, col: 0, last_col: 0 },
                CellInfo { row: 0, last_row: 0, col: 1, last_col: 1 },
                CellInfo { row: 1, last_row: 1, col: 0, last_col: 0 },
                CellInfo { row: 1, last_row: 1, col: 1, last_col: 1 },
            ],
        }
    }
    fn dimension_properties(ends: &[u32], sizes: &[u32], width: u32, height: u32) -> Vec<u8> {
        fn number(key: u8, value: u32) -> Vec<u8> {
            [vec![key, 0x20], value.to_le_bytes().to_vec()].concat()
        }
        fn list(key: u8, kind: u8, data: &[u8]) -> Vec<u8> {
            [vec![key, kind], ((data.len() + 4) as u32).to_le_bytes().to_vec(), data.to_vec()].concat()
        }
        let mut entries = Vec::new();
        for (end, size) in [(1_782_000, 1_782_000), (3_564_000, 1_782_000)].into_iter().chain(ends.iter().copied().zip(sizes.iter().copied())) {
            entries.extend(list(0, 0x88, &[number(1, end), number(2, size)].concat()));
        }
        [number(0x66, sizes.len() as u32), number(0x67, 2), number(0x68, width), number(0x69, height), number(0x6b, 260), list(0x6d, 0x90, &entries)]
            .concat()
    }
    #[test]
    fn row_endings_preserve_rejection_of_invalid_grids_totals_and_overflow() {
        for ends in [[219_188, 219_188, 657_564], [219_188, 219_187, 657_564], [0, 438_376, 657_564], [u32::MAX, 438_376, 657_564]] {
            let bytes = dimension_properties(&ends, &[216_000; 3], 3_564_000, 648_000);
            assert!(TableInfo::parse(&properties(&bytes).unwrap()).is_err());
        }
        for bytes in [
            dimension_properties(&[219_188, 438_376, 657_564], &[216_000; 3], 3_564_000, 648_001),
            dimension_properties(&[219_188, 438_376, 657_564], &[216_000; 3], 3_564_001, 648_000),
            dimension_properties(&[219_188, 438_376], &[216_000; 3], 3_564_000, 648_000),
            dimension_properties(&[219_188, 438_376, 657_564], &[0; 3], 3_564_000, 0),
        ] {
            assert!(TableInfo::parse(&properties(&bytes).unwrap()).is_err());
        }
        let mut bytes = dimension_properties(&[219_188, 438_376, 657_564], &[216_000; 3], 3_564_000, 648_000);
        let marker = [vec![1, 0x20], 1_782_000u32.to_le_bytes().to_vec()].concat();
        let at = bytes.windows(marker.len()).position(|w| w == marker).unwrap() + 2;
        bytes[at..at + 4].copy_from_slice(&1_782_001u32.to_le_bytes());
        assert!(TableInfo::parse(&properties(&bytes).unwrap()).is_err());
        let bytes = dimension_properties(&(1..=32).collect::<Vec<_>>(), &[197_510_400; 32], 3_564_000, 0);
        let error = TableInfo::parse(&properties(&bytes).unwrap()).err().unwrap();
        assert!(error.to_string().contains("dimension overflow"));
    }
    #[test]
    fn rejects_overlapping_missing_and_out_of_bounds_cells() {
        let mut info = grid();
        info.cells.push(info.cells[0].clone());
        assert!(info.validate().is_err());
        info.cells = vec![];
        assert!(info.validate().is_err());
        info.cells.push(CellInfo { row: 0, last_row: usize::MAX, col: 0, last_col: 0 });
        assert!(info.validate().is_err());
        let mut bytes = vec![0x66, 0x22];
        bytes.extend(u32::MAX.to_le_bytes());
        bytes.extend([0x67, 0x22, 2, 0, 0, 0, 0x6b, 0x70, 4, 1, 0, 0]);
        assert!(TableInfo::parse(&properties(&bytes).unwrap()).is_err());
    }
}

#[cfg(test)]
mod multi_story_text_tests {
    use super::*;
    fn example() -> (crate::contents::Contents, crate::quill::Quill, Vec<u8>) {
        let mut c = crate::contents::Contents::default();
        c.shapes.insert(
            42,
            crate::contents::ShapeInfo {
                is_table: true,
                text_id: Some(6),
                table: Some(TableInfo {
                    rows: vec![457200],
                    columns: vec![1828800; 2],
                    cells: (0..2).map(|col| CellInfo { row: 0, last_row: 0, col, last_col: col }).collect(),
                    cell_record: 0,
                    row_ending_difference: 0,
                }),
                ..Default::default()
            },
        );
        let mut q = crate::quill::Quill::default();
        for (id, text) in [(1, "Other\r"), (6, "AB\rCD\r")] {
            q.stories.insert(id, crate::quill::QStory { units: text.encode_utf16().collect(), ..Default::default() });
        }
        q.warnings = vec!["Publisher table text across multiple stories/TCD sections is not decoded.".into(), "another real loss".into()];
        let mut bytes = vec![0; 200];
        bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
        bytes[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
        bytes[34..38].copy_from_slice(b"TCD ");
        bytes[48..52].copy_from_slice(&96u32.to_le_bytes());
        bytes[52..56].copy_from_slice(&20u32.to_le_bytes());
        for (i, n) in [1u32, 0, 0xff00, 2, 6].iter().enumerate() {
            bytes[96 + 4 * i..100 + 4 * i].copy_from_slice(&n.to_le_bytes());
        }
        (c, q, bytes)
    }
    #[test]
    fn explicit_table_link_and_unique_boundaries_recover_cell_ranges() {
        let (c, mut q, bytes) = example();
        recover_single_table_text(&c, &mut q, &bytes).unwrap();
        assert_eq!(q.stories[&6].cell_ranges, Some(vec![0..2, 3..5]));
        assert!(q.stories[&1].cell_ranges.is_none());
        assert_eq!(q.warnings, ["another real loss"]);
    }
    #[test]
    fn uncertain_associations_preserve_text_and_loss_warning() {
        for case in 0..6 {
            let (mut c, mut q, mut bytes) = example();
            match case {
                0 => q.stories.get_mut(&1).unwrap().units = "AB\rCD\r".encode_utf16().collect(),
                1 => c.shapes.get_mut(&42).unwrap().text_id = Some(7),
                2 => {
                    bytes[26..28].copy_from_slice(&2u16.to_le_bytes());
                    let entry = bytes[32..56].to_vec();
                    bytes[56..80].copy_from_slice(&entry);
                }
                3 => bytes[104..108].copy_from_slice(&0u32.to_le_bytes()),
                4 => c.shapes.get_mut(&42).unwrap().table.as_mut().unwrap().cells.swap(0, 1),
                _ => bytes[112..116].copy_from_slice(&5u32.to_le_bytes()),
            }
            let original: Vec<_> = q.stories.iter().map(|(id, s)| (*id, s.units.clone())).collect();
            recover_single_table_text(&c, &mut q, &bytes).unwrap();
            assert!(q.stories.values().all(|s| s.cell_ranges.is_none()), "case {case}");
            for (id, units) in original {
                assert_eq!(q.stories[&id].units, units);
            }
            assert_eq!(q.warnings.len(), 2, "case {case}");
        }
    }
    #[test]
    fn repeated_directory_offsets_are_rejected() {
        let (_, _, mut bytes) = example();
        bytes[28..32].copy_from_slice(&24u32.to_le_bytes());
        assert!(single_tcd(&bytes).is_err());
    }
}
