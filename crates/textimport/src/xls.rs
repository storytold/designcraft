//! BIFF8 `.xls` workbooks for data merge. The reader understands an OLE compound file and the
//! worksheet records a data source needs: shared strings, labels, numbers, RK, and a formula's
//! cached number or string. Place-as-table does not use this module.
//!
//! A shared-string CONTINUE is joined onto the string table. That is correct when the split falls
//! between strings. A CONTINUE that begins in the middle of a character is not reconstructed.

use std::collections::HashSet;

use crate::ImportError;

const SIG: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
const ENDOFCHAIN: u32 = 0xFFFF_FFFE;
const FREESECT: u32 = 0xFFFF_FFFF;
const FATSECT: u32 = 0xFFFF_FFFD;
const MAX_STREAM: usize = 64 * 1024 * 1024;
const HEADER: usize = 512;
const SECTOR: usize = 512;
const MINI: usize = 64;
const CUTOFF: usize = 4096;

/// One cell in [`xls_fixture`].
#[derive(Clone, Debug, PartialEq)]
pub enum XlsCell {
    Empty,
    Text(String),
    Number(f64),
    /// A 30-bit integer stored as an RK value.
    Rk(i32),
    /// A formula cell whose cached result is this number.
    Formula(f64),
}

/// Rows of a BIFF8 workbook, with the same grid rules as an `.xlsx` data source.
pub fn records(bytes: &[u8]) -> Result<Vec<Vec<String>>, ImportError> {
    let stream = workbook_stream(bytes)?;
    let cells = biff_cells(&stream)?;
    super::xlsx::assemble(cells)
}

/// A small `.xls` file. Streams under 4096 bytes are stored in the mini stream, as a real
/// workbook stores them.
pub fn xls_fixture(rows: &[Vec<XlsCell>]) -> Vec<u8> {
    compound(&biff_sheet(rows))
}

fn workbook_stream(bytes: &[u8]) -> Result<Vec<u8>, ImportError> {
    if bytes.get(..8) != Some(&SIG) {
        return Err(not_xls());
    }
    let major = u16_at(bytes, 0x1A).ok_or_else(not_xls)?;
    if major != 3 && major != 4 {
        return Err(not_xls());
    }
    if u16_at(bytes, 0x1C) != Some(0xFFFE) {
        return Err(not_xls());
    }
    let shift = u16_at(bytes, 0x1E).ok_or_else(not_xls)?;
    if !(9..=12).contains(&shift) {
        return Err(not_xls());
    }
    let sector = 1usize << shift;
    let header = if major >= 4 { sector } else { HEADER };
    if bytes.len() < header {
        return Err(not_xls());
    }
    let mini_shift = u16_at(bytes, 0x20).unwrap_or(6);
    if !(4..=12).contains(&mini_shift) {
        return Err(not_xls());
    }
    let mini = 1usize << mini_shift;
    let cutoff = u32_at(bytes, 0x38).unwrap_or(CUTOFF as u32) as usize;
    let fat = read_fat(bytes, header, sector)?;
    let dir_start = u32_at(bytes, 0x30).ok_or_else(not_xls)?;
    let dir = read_chain(bytes, header, sector, &fat, dir_start, MAX_STREAM)?;
    let mut root: Option<(u32, u64)> = None;
    let mut book: Option<(u32, u64)> = None;
    let mut at = 0;
    while at + 128 <= dir.len() {
        let entry = &dir[at..at + 128];
        at += 128;
        let ty = entry[0x42];
        if ty == 0 {
            continue;
        }
        let name = entry_name(entry);
        let start = u32_at(entry, 0x74).unwrap_or(FREESECT);
        let size = if major >= 4 { u64_at(entry, 0x78).unwrap_or(0) } else { u64::from(u32_at(entry, 0x78).unwrap_or(0)) };
        if ty == 5 {
            root = Some((start, size));
        } else if ty == 2 && is_book_name(&name) {
            book = Some((start, size));
        }
    }
    let (start, size) = book.ok_or_else(not_xls)?;
    if size > MAX_STREAM as u64 {
        return Err(ImportError::Corrupt("the workbook is too large".into()));
    }
    let size = size as usize;
    if size >= cutoff {
        return read_chain_len(bytes, header, sector, &fat, start, size);
    }
    let (root_start, root_size) = root.ok_or_else(not_xls)?;
    if root_size > MAX_STREAM as u64 {
        return Err(ImportError::Corrupt("the workbook is too large".into()));
    }
    let mini_fat_start = u32_at(bytes, 0x3C).unwrap_or(ENDOFCHAIN);
    let mini_fat_bytes =
        if mini_fat_start >= 0xFFFF_FFF0 { Vec::new() } else { read_chain(bytes, header, sector, &fat, mini_fat_start, MAX_STREAM)? };
    let mini_fat = u32_list(&mini_fat_bytes);
    let container = read_chain_len(bytes, header, sector, &fat, root_start, root_size as usize)?;
    read_mini(&container, &mini_fat, start, size, mini)
}

fn read_fat(bytes: &[u8], header: usize, sector: usize) -> Result<Vec<u32>, ImportError> {
    let mut sectors = Vec::new();
    for i in 0..109 {
        let v = u32_at(bytes, 0x4C + i * 4).unwrap_or(FREESECT);
        if v >= 0xFFFF_FFF0 {
            break;
        }
        sectors.push(v);
    }
    let mut next = u32_at(bytes, 0x44).unwrap_or(ENDOFCHAIN);
    let mut guard = 0u32;
    let mut seen = HashSet::new();
    while next < 0xFFFF_FFF0 && guard < 1024 {
        if !seen.insert(next) {
            return Err(not_xls());
        }
        guard += 1;
        let off = sector_off(header, sector, next).ok_or_else(not_xls)?;
        let block = bytes.get(off..off + sector).ok_or_else(not_xls)?;
        let per = sector / 4;
        if per < 2 {
            return Err(not_xls());
        }
        for i in 0..per - 1 {
            let v = u32_at(block, i * 4).unwrap_or(FREESECT);
            if v >= 0xFFFF_FFF0 {
                break;
            }
            sectors.push(v);
        }
        next = u32_at(block, (per - 1) * 4).unwrap_or(ENDOFCHAIN);
    }
    if sectors.is_empty() || sectors.len() > 4096 {
        return Err(not_xls());
    }
    let mut fat = Vec::new();
    let mut seen_fat = HashSet::new();
    for id in sectors {
        if !seen_fat.insert(id) {
            return Err(not_xls());
        }
        let off = sector_off(header, sector, id).ok_or_else(not_xls)?;
        let block = bytes.get(off..off + sector).ok_or_else(not_xls)?;
        fat.extend(u32_list(block));
    }
    Ok(fat)
}

fn read_chain(bytes: &[u8], header: usize, sector: usize, fat: &[u32], start: u32, cap: usize) -> Result<Vec<u8>, ImportError> {
    if start >= 0xFFFF_FFF0 {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut id = start;
    let mut guard = 0u32;
    while id < 0xFFFF_FFF0 {
        if !seen.insert(id) || guard > 1_000_000 {
            return Err(not_xls());
        }
        guard += 1;
        let off = sector_off(header, sector, id).ok_or_else(not_xls)?;
        let block = bytes.get(off..off + sector).ok_or_else(not_xls)?;
        if out.len().saturating_add(block.len()) > cap {
            return Err(ImportError::Corrupt("the workbook is too large".into()));
        }
        out.extend_from_slice(block);
        id = fat.get(id as usize).copied().ok_or_else(not_xls)?;
    }
    Ok(out)
}

fn read_chain_len(bytes: &[u8], header: usize, sector: usize, fat: &[u32], start: u32, size: usize) -> Result<Vec<u8>, ImportError> {
    if size > MAX_STREAM {
        return Err(ImportError::Corrupt("the workbook is too large".into()));
    }
    let mut data = read_chain(bytes, header, sector, fat, start, MAX_STREAM)?;
    if data.len() < size {
        return Err(not_xls());
    }
    data.truncate(size);
    Ok(data)
}

fn read_mini(stream: &[u8], fat: &[u32], start: u32, size: usize, mini: usize) -> Result<Vec<u8>, ImportError> {
    if size == 0 {
        return Ok(Vec::new());
    }
    if mini == 0 || start >= 0xFFFF_FFF0 {
        return Err(not_xls());
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut id = start;
    while out.len() < size && id < 0xFFFF_FFF0 {
        if !seen.insert(id) {
            return Err(not_xls());
        }
        let off = (id as usize).checked_mul(mini).ok_or_else(not_xls)?;
        let end = off.saturating_add(mini).min(stream.len());
        let block = stream.get(off..end).ok_or_else(not_xls)?;
        if block.is_empty() {
            return Err(not_xls());
        }
        let need = size - out.len();
        let take = block.len().min(need);
        out.extend_from_slice(block.get(..take).ok_or_else(not_xls)?);
        id = fat.get(id as usize).copied().unwrap_or(ENDOFCHAIN);
    }
    if out.len() < size {
        return Err(not_xls());
    }
    Ok(out)
}

fn biff_cells(data: &[u8]) -> Result<Vec<(usize, usize, String)>, ImportError> {
    let mut i = 0;
    let mut sst = Vec::new();
    let mut cells = Vec::new();
    let mut pending: Option<(usize, usize)> = None;
    while i + 4 <= data.len() {
        let kind = u16_at(data, i).unwrap_or(0);
        let len = usize::from(u16_at(data, i + 2).unwrap_or(0));
        if kind == 0 && len == 0 {
            break;
        }
        let start = i + 4;
        let end = start.saturating_add(len);
        if end > data.len() {
            return Err(ImportError::Corrupt("a BIFF record is truncated".into()));
        }
        let mut payload = data[start..end].to_vec();
        i = end;
        if kind == 0x00FC {
            while i + 4 <= data.len() && u16_at(data, i) == Some(0x003C) {
                let extra = usize::from(u16_at(data, i + 2).unwrap_or(0));
                let s2 = i + 4;
                let e2 = s2.saturating_add(extra);
                if e2 > data.len() {
                    return Err(ImportError::Corrupt("a BIFF record is truncated".into()));
                }
                payload.extend_from_slice(&data[s2..e2]);
                i = e2;
            }
            sst = parse_sst(&payload)?;
            pending = None;
            continue;
        }
        if kind == 0x0207 {
            if let Some((row, col)) = pending.take()
                && let Some((text, _)) = xl_unicode(&payload)
            {
                cells.push((row, col, text));
            }
            continue;
        }
        pending = None;
        match kind {
            0x00FD => push_labelsst(&payload, &sst, &mut cells),
            0x0204 => push_label(&payload, &mut cells),
            0x0203 => push_number(&payload, &mut cells),
            0x027E => push_rk(&payload, &mut cells),
            0x00BD => push_mulrk(&payload, &mut cells),
            0x0006 => pending = push_formula(&payload, &mut cells),
            _ => {}
        }
    }
    if cells.is_empty() {
        return Err(ImportError::Corrupt("the worksheet is empty".into()));
    }
    Ok(cells)
}

fn push_labelsst(payload: &[u8], sst: &[String], cells: &mut Vec<(usize, usize, String)>) {
    let Some(row) = u16_at(payload, 0) else { return };
    let Some(col) = u16_at(payload, 2) else { return };
    let Some(idx) = u32_at(payload, 6) else { return };
    let text = sst.get(idx as usize).cloned().unwrap_or_default();
    cells.push((usize::from(row), usize::from(col), text));
}

fn push_label(payload: &[u8], cells: &mut Vec<(usize, usize, String)>) {
    let Some(row) = u16_at(payload, 0) else { return };
    let Some(col) = u16_at(payload, 2) else { return };
    let Some(rest) = payload.get(6..) else { return };
    let Some((text, _)) = xl_unicode(rest) else { return };
    cells.push((usize::from(row), usize::from(col), text));
}

fn push_number(payload: &[u8], cells: &mut Vec<(usize, usize, String)>) {
    let Some((row, col, n)) = number_at(payload, 6) else { return };
    cells.push((row, col, num_text(n)));
}

fn push_rk(payload: &[u8], cells: &mut Vec<(usize, usize, String)>) {
    let Some(row) = u16_at(payload, 0) else { return };
    let Some(col) = u16_at(payload, 2) else { return };
    let Some(rk) = u32_at(payload, 6) else { return };
    let n = decode_rk(rk);
    if n.is_finite() {
        cells.push((usize::from(row), usize::from(col), num_text(n)));
    }
}

fn push_mulrk(payload: &[u8], cells: &mut Vec<(usize, usize, String)>) {
    let Some(row) = u16_at(payload, 0) else { return };
    let Some(col0) = u16_at(payload, 2) else { return };
    if payload.len() < 6 {
        return;
    }
    let body = payload.len() - 2;
    let mut i = 4;
    let mut col = u32::from(col0);
    while i + 6 <= body {
        let Some(rk) = u32_at(payload, i + 2) else { return };
        let n = decode_rk(rk);
        if n.is_finite() {
            cells.push((usize::from(row), col as usize, num_text(n)));
        }
        col = col.saturating_add(1);
        i += 6;
    }
}

/// A numeric cache is stored. A string cache returns the cell so the following STRING record can fill it.
fn push_formula(payload: &[u8], cells: &mut Vec<(usize, usize, String)>) -> Option<(usize, usize)> {
    let row = u16_at(payload, 0)?;
    let col = u16_at(payload, 2)?;
    let result = payload.get(6..14)?;
    if result.get(6) == Some(&0xFF) && result.get(7) == Some(&0xFF) {
        return match result.first().copied() {
            Some(0) => Some((usize::from(row), usize::from(col))),
            Some(1) => {
                let text = if result.get(1) == Some(&1) { "TRUE" } else { "FALSE" };
                cells.push((usize::from(row), usize::from(col), text.to_string()));
                None
            }
            _ => None,
        };
    }
    let n = f64_at(payload, 6)?;
    if n.is_finite() {
        cells.push((usize::from(row), usize::from(col), num_text(n)));
    }
    None
}

fn number_at(payload: &[u8], at: usize) -> Option<(usize, usize, f64)> {
    let row = u16_at(payload, 0)?;
    let col = u16_at(payload, 2)?;
    let n = f64_at(payload, at)?;
    n.is_finite().then_some((usize::from(row), usize::from(col), n))
}

fn parse_sst(buf: &[u8]) -> Result<Vec<String>, ImportError> {
    if buf.len() < 8 {
        return Err(ImportError::Corrupt("the shared string table is truncated".into()));
    }
    let unique = (u32_at(buf, 4).unwrap_or(0) as usize).min(200_000);
    let mut out = Vec::new();
    let mut i = 8;
    while out.len() < unique && i + 3 <= buf.len() {
        let cch = usize::from(u16_at(buf, i).unwrap_or(0));
        let flags = buf[i + 2];
        i += 3;
        let mut runs = 0usize;
        let mut ext = 0usize;
        if flags & 0x08 != 0 {
            let Some(n) = u16_at(buf, i) else { break };
            runs = usize::from(n);
            i += 2;
        }
        if flags & 0x04 != 0 {
            let Some(n) = u32_at(buf, i) else { break };
            ext = n as usize;
            i += 4;
        }
        let nbytes = if flags & 0x01 != 0 { cch.saturating_mul(2) } else { cch };
        let Some(data) = buf.get(i..i + nbytes) else { break };
        let text = if flags & 0x01 != 0 { utf16_le(data) } else { String::from_utf8_lossy(data).into_owned() };
        i += nbytes;
        let skip = runs.saturating_mul(4).saturating_add(ext);
        if i + skip > buf.len() {
            out.push(text);
            break;
        }
        i += skip;
        out.push(text);
    }
    Ok(out)
}

fn xl_unicode(buf: &[u8]) -> Option<(String, usize)> {
    if buf.len() < 3 {
        return None;
    }
    let cch = usize::from(u16_at(buf, 0)?);
    let flags = buf[2];
    let nbytes = if flags & 1 != 0 { cch.saturating_mul(2) } else { cch };
    let data = buf.get(3..3 + nbytes)?;
    let text = if flags & 1 != 0 { utf16_le(data) } else { String::from_utf8_lossy(data).into_owned() };
    Some((text, 3 + nbytes))
}

fn utf16_le(bytes: &[u8]) -> String {
    let mut units = Vec::with_capacity(bytes.len() / 2);
    let mut i = 0;
    while i + 1 < bytes.len() {
        let Some(pair) = bytes.get(i..i + 2) else { break };
        units.push(u16::from_le_bytes([pair[0], pair[1]]));
        i += 2;
    }
    String::from_utf16_lossy(&units)
}

/// RK bits. Bit 0 divides by 100. Bit 1 selects a 30-bit integer; otherwise the high 30 bits of a float.
pub(crate) fn decode_rk(rk: u32) -> f64 {
    let value = if rk & 2 != 0 { f64::from((rk as i32) >> 2) } else { f64::from_bits(u64::from(rk & 0xFFFF_FFFC) << 32) };
    if rk & 1 != 0 { value / 100.0 } else { value }
}

fn num_text(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 { format!("{}", f as i64) } else { format!("{}", (f * 1e10).round() / 1e10) }
}

fn not_xls() -> ImportError {
    ImportError::Corrupt("not an .xls workbook".into())
}

fn is_book_name(name: &str) -> bool {
    let n = name.trim();
    n.eq_ignore_ascii_case("workbook") || n.eq_ignore_ascii_case("book")
}

fn entry_name(entry: &[u8]) -> String {
    let nbytes = usize::from(u16_at(entry, 0x40).unwrap_or(0)).min(64);
    if nbytes < 2 {
        return String::new();
    }
    let Some(data) = entry.get(..nbytes) else { return String::new() };
    let mut units = Vec::new();
    let mut i = 0;
    while i + 1 < data.len() {
        let u = u16::from_le_bytes([data[i], data[i + 1]]);
        if u == 0 {
            break;
        }
        units.push(u);
        i += 2;
    }
    String::from_utf16_lossy(&units)
}

fn u32_list(bytes: &[u8]) -> Vec<u32> {
    let mut out = Vec::with_capacity(bytes.len() / 4);
    let mut i = 0;
    while i + 4 <= bytes.len() {
        if let Some(n) = u32_at(bytes, i) {
            out.push(n);
        }
        i += 4;
    }
    out
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    let s = bytes.get(at..at + 2)?;
    Some(u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let s = bytes.get(at..at + 4)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    let s = bytes.get(at..at + 8)?;
    let mut a = [0u8; 8];
    a.copy_from_slice(s);
    Some(u64::from_le_bytes(a))
}

fn f64_at(bytes: &[u8], at: usize) -> Option<f64> {
    let s = bytes.get(at..at + 8)?;
    let mut a = [0u8; 8];
    a.copy_from_slice(s);
    Some(f64::from_le_bytes(a))
}

fn sector_off(header: usize, sector: usize, id: u32) -> Option<usize> {
    (id as usize).checked_mul(sector)?.checked_add(header)
}

fn biff_sheet(rows: &[Vec<XlsCell>]) -> Vec<u8> {
    let mut strings: Vec<String> = Vec::new();
    let mut total = 0u32;
    for (r, row) in rows.iter().take(10_000).enumerate() {
        for (c, cell) in row.iter().take(256).enumerate() {
            if r > usize::from(u16::MAX) || c > usize::from(u16::MAX) {
                continue;
            }
            if c == 0
                && let XlsCell::Text(text) = cell
            {
                total = total.saturating_add(1);
                if !strings.iter().any(|s| s == text) {
                    strings.push(text.clone());
                }
            }
        }
    }
    let mut out = Vec::new();
    push_record(&mut out, 0x0809, &bof(0x0005));
    push_sst(&mut out, &strings, total);
    push_record(&mut out, 0x000A, &[]);
    push_record(&mut out, 0x0809, &bof(0x0010));
    for (r, row) in rows.iter().take(10_000).enumerate() {
        let Some(row_n) = u16::try_from(r).ok() else { continue };
        for (c, cell) in row.iter().take(256).enumerate() {
            let Some(col_n) = u16::try_from(c).ok() else { continue };
            match cell {
                XlsCell::Empty => {}
                XlsCell::Text(text) if c == 0 => {
                    let idx = strings.iter().position(|s| s == text).unwrap_or(0) as u32;
                    push_record(&mut out, 0x00FD, &labelsst(row_n, col_n, idx));
                }
                XlsCell::Text(text) => push_record(&mut out, 0x0204, &label(row_n, col_n, text)),
                XlsCell::Number(n) => push_record(&mut out, 0x0203, &number_rec(row_n, col_n, *n)),
                XlsCell::Rk(n) => {
                    if let Some(rk) = encode_rk(*n) {
                        push_record(&mut out, 0x027E, &rk_rec(row_n, col_n, rk));
                    } else {
                        push_record(&mut out, 0x0203, &number_rec(row_n, col_n, f64::from(*n)));
                    }
                }
                XlsCell::Formula(n) => push_record(&mut out, 0x0006, &formula_rec(row_n, col_n, *n)),
            }
            if out.len() > 1_048_576 {
                break;
            }
        }
    }
    push_record(&mut out, 0x000A, &[]);
    out.extend_from_slice(&[0, 0, 0, 0]);
    out
}

fn bof(dt: u16) -> [u8; 16] {
    let mut p = [0u8; 16];
    p[0..2].copy_from_slice(&0x0600u16.to_le_bytes());
    p[2..4].copy_from_slice(&dt.to_le_bytes());
    p
}

fn push_sst(out: &mut Vec<u8>, strings: &[String], total: u32) {
    if strings.is_empty() {
        return;
    }
    let encoded: Vec<Vec<u8>> = strings.iter().map(|s| encode_xl(s)).collect();
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    let mut cur = Vec::new();
    cur.extend_from_slice(&total.to_le_bytes());
    cur.extend_from_slice(&(u32::try_from(strings.len()).unwrap_or(0)).to_le_bytes());
    for enc in &encoded {
        if cur.len() + enc.len() > 8000 && cur.len() > 8 {
            chunks.push(std::mem::take(&mut cur));
        }
        cur.extend_from_slice(enc);
    }
    chunks.push(cur);
    if let Some(first) = chunks.first() {
        push_record(out, 0x00FC, first);
    }
    for extra in chunks.iter().skip(1) {
        push_record(out, 0x003C, extra);
    }
}

fn encode_xl(text: &str) -> Vec<u8> {
    let units: Vec<u16> = text.encode_utf16().take(32_000).collect();
    let cch = u16::try_from(units.len()).unwrap_or(u16::MAX);
    let units = &units[..usize::from(cch).min(units.len())];
    let mut v = Vec::with_capacity(3 + units.len() * 2);
    v.extend_from_slice(&cch.to_le_bytes());
    v.push(1);
    for u in units {
        v.extend_from_slice(&u.to_le_bytes());
    }
    v
}

fn labelsst(row: u16, col: u16, idx: u32) -> Vec<u8> {
    let mut p = Vec::with_capacity(10);
    p.extend_from_slice(&row.to_le_bytes());
    p.extend_from_slice(&col.to_le_bytes());
    p.extend_from_slice(&0u16.to_le_bytes());
    p.extend_from_slice(&idx.to_le_bytes());
    p
}

fn label(row: u16, col: u16, text: &str) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&row.to_le_bytes());
    p.extend_from_slice(&col.to_le_bytes());
    p.extend_from_slice(&0u16.to_le_bytes());
    p.extend_from_slice(&encode_xl(text));
    p
}

fn number_rec(row: u16, col: u16, n: f64) -> Vec<u8> {
    let mut p = Vec::with_capacity(14);
    p.extend_from_slice(&row.to_le_bytes());
    p.extend_from_slice(&col.to_le_bytes());
    p.extend_from_slice(&0u16.to_le_bytes());
    p.extend_from_slice(&n.to_le_bytes());
    p
}

fn rk_rec(row: u16, col: u16, rk: u32) -> Vec<u8> {
    let mut p = Vec::with_capacity(10);
    p.extend_from_slice(&row.to_le_bytes());
    p.extend_from_slice(&col.to_le_bytes());
    p.extend_from_slice(&0u16.to_le_bytes());
    p.extend_from_slice(&rk.to_le_bytes());
    p
}

fn formula_rec(row: u16, col: u16, n: f64) -> Vec<u8> {
    let mut p = vec![0u8; 22];
    p[0..2].copy_from_slice(&row.to_le_bytes());
    p[2..4].copy_from_slice(&col.to_le_bytes());
    p[6..14].copy_from_slice(&n.to_le_bytes());
    p
}

fn encode_rk(n: i32) -> Option<u32> {
    if !(-0x2000_0000..0x2000_0000).contains(&n) {
        return None;
    }
    Some(n.wrapping_shl(2) as u32 | 2)
}

fn push_record(out: &mut Vec<u8>, kind: u16, payload: &[u8]) {
    let Ok(len) = u16::try_from(payload.len()) else { return };
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(payload);
}

fn compound(biff: &[u8]) -> Vec<u8> {
    if biff.len() >= CUTOFF { compound_regular(biff) } else { compound_mini(biff) }
}

fn compound_mini(biff: &[u8]) -> Vec<u8> {
    let mini_count = biff.len().div_ceil(MINI).clamp(1, 120);
    let mini_bytes = mini_count * MINI;
    let containers = mini_bytes.div_ceil(SECTOR);
    let total = 3 + containers;
    let mut file = vec![0u8; HEADER + total * SECTOR];
    write_header(&mut file, 1, true);
    let mut fat = vec![FREESECT; 128];
    fat[0] = FATSECT;
    fat[1] = ENDOFCHAIN;
    fat[2] = ENDOFCHAIN;
    for i in 0..containers {
        let sector = 3 + i;
        fat[sector] = if i + 1 == containers { ENDOFCHAIN } else { (sector + 1) as u32 };
    }
    write_fat(&mut file, 0, &fat);
    let root = dir_entry("Root Entry", 5, 1, 3, mini_bytes as u32);
    let book = dir_entry("Workbook", 2, FREESECT, 0, biff.len().min(mini_bytes) as u32);
    write_dir(&mut file, &root, &book);
    let mut mini_fat = vec![FREESECT; 128];
    for (i, slot) in mini_fat.iter_mut().take(mini_count).enumerate() {
        *slot = if i + 1 == mini_count { ENDOFCHAIN } else { (i + 1) as u32 };
    }
    write_fat(&mut file, 2, &mini_fat);
    let data_at = HEADER + 3 * SECTOR;
    let n = biff.len().min(mini_bytes);
    if let Some(dst) = file.get_mut(data_at..data_at + n) {
        dst.copy_from_slice(&biff[..n]);
    }
    file
}

fn compound_regular(biff: &[u8]) -> Vec<u8> {
    let data_sectors = biff.len().div_ceil(SECTOR).clamp(1, 120);
    let kept = data_sectors * SECTOR;
    let total = 2 + data_sectors;
    let mut file = vec![0u8; HEADER + total * SECTOR];
    write_header(&mut file, 1, false);
    let mut fat = vec![FREESECT; 128];
    fat[0] = FATSECT;
    fat[1] = ENDOFCHAIN;
    for i in 0..data_sectors {
        let sector = 2 + i;
        fat[sector] = if i + 1 == data_sectors { ENDOFCHAIN } else { (sector + 1) as u32 };
    }
    write_fat(&mut file, 0, &fat);
    let root = dir_entry("Root Entry", 5, 1, ENDOFCHAIN, 0);
    let book = dir_entry("Workbook", 2, FREESECT, 2, biff.len().min(kept) as u32);
    write_dir(&mut file, &root, &book);
    let data_at = HEADER + 2 * SECTOR;
    let n = biff.len().min(kept);
    if let Some(dst) = file.get_mut(data_at..data_at + n) {
        dst.copy_from_slice(&biff[..n]);
    }
    file
}

fn write_header(file: &mut [u8], fat_sectors: u32, mini: bool) {
    if let Some(sig) = file.get_mut(..8) {
        sig.copy_from_slice(&SIG);
    }
    put_u16(file, 0x18, 0x003E);
    put_u16(file, 0x1A, 3);
    put_u16(file, 0x1C, 0xFFFE);
    put_u16(file, 0x1E, 9);
    put_u16(file, 0x20, 6);
    put_u32(file, 0x2C, fat_sectors);
    put_u32(file, 0x30, 1);
    put_u32(file, 0x38, CUTOFF as u32);
    if mini {
        put_u32(file, 0x3C, 2);
        put_u32(file, 0x40, 1);
    } else {
        put_u32(file, 0x3C, ENDOFCHAIN);
        put_u32(file, 0x40, 0);
    }
    put_u32(file, 0x44, ENDOFCHAIN);
    put_u32(file, 0x48, 0);
    for i in 0..109 {
        put_u32(file, 0x4C + i * 4, FREESECT);
    }
    put_u32(file, 0x4C, 0);
}

fn write_fat(file: &mut [u8], sector: usize, entries: &[u32]) {
    let at = HEADER + sector * SECTOR;
    for (i, v) in entries.iter().take(128).enumerate() {
        put_u32(file, at + i * 4, *v);
    }
}

fn write_dir(file: &mut [u8], root: &[u8; 128], book: &[u8; 128]) {
    let at = HEADER + SECTOR;
    if let Some(dst) = file.get_mut(at..at + 128) {
        dst.copy_from_slice(root);
    }
    if let Some(dst) = file.get_mut(at + 128..at + 256) {
        dst.copy_from_slice(book);
    }
}

fn dir_entry(name: &str, ty: u8, child: u32, start: u32, size: u32) -> [u8; 128] {
    let mut e = [0u8; 128];
    let mut units: Vec<u16> = name.encode_utf16().take(31).collect();
    units.push(0);
    for (i, u) in units.iter().enumerate() {
        let at = i * 2;
        if at + 1 < 64 {
            e[at] = (u & 0xFF) as u8;
            e[at + 1] = (u >> 8) as u8;
        }
    }
    let nbytes = (units.len() * 2).min(64) as u16;
    e[0x40..0x42].copy_from_slice(&nbytes.to_le_bytes());
    e[0x42] = ty;
    e[0x43] = 1;
    e[0x44..0x4C].fill(0xFF);
    e[0x4C..0x50].copy_from_slice(&child.to_le_bytes());
    e[0x74..0x78].copy_from_slice(&start.to_le_bytes());
    e[0x78..0x7C].copy_from_slice(&size.to_le_bytes());
    e
}

fn put_u16(buf: &mut [u8], at: usize, v: u16) {
    if let Some(dst) = buf.get_mut(at..at + 2) {
        dst.copy_from_slice(&v.to_le_bytes());
    }
}

fn put_u32(buf: &mut [u8], at: usize, v: u32) {
    if let Some(dst) = buf.get_mut(at..at + 4) {
        dst.copy_from_slice(&v.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rk_twelve_is_decoded_from_hand_built_bytes() {
        let raw = [50u8, 0, 0, 0];
        let rk = u32::from_le_bytes(raw);
        assert_eq!(decode_rk(rk), 12.0);
        assert_eq!(decode_rk(51), 0.12);
    }

    #[test]
    fn fixture_roundtrip_keeps_rows_and_uses_the_mini_stream() {
        let rows = vec![
            vec![XlsCell::Text("Name".into()), XlsCell::Text("Qty".into()), XlsCell::Text("Note".into())],
            vec![XlsCell::Text("Ada".into()), XlsCell::Rk(12), XlsCell::Formula(9.0)],
            vec![XlsCell::Text("Bea".into()), XlsCell::Empty, XlsCell::Empty],
            vec![XlsCell::Empty, XlsCell::Empty, XlsCell::Empty],
            vec![XlsCell::Text("Cy".into()), XlsCell::Number(7.5), XlsCell::Text("ok".into())],
        ];
        let bytes = xls_fixture(&rows);
        assert!(bytes.len() < 4096, "a short workbook stays in the mini stream, len {}", bytes.len());
        assert_eq!(bytes.get(..8), Some(&SIG[..]));
        let got = records(&bytes).unwrap();
        assert_eq!(got[0], vec!["Name".to_string(), "Qty".to_string(), "Note".to_string()]);
        assert_eq!(got[1], vec!["Ada".to_string(), "12".to_string(), "9".to_string()]);
        assert_eq!(got[2], vec!["Bea".to_string(), String::new(), String::new()]);
        assert_eq!(got[3], vec!["Cy".to_string(), "7.5".to_string(), "ok".to_string()]);
        assert_eq!(got.len(), 4, "a fully empty data row is dropped");
    }

    #[test]
    fn mulrk_and_a_formula_string_are_read_without_the_fixture_writer() {
        let mut biff = Vec::new();
        push_record(&mut biff, 0x0809, &bof(0x0005));
        push_record(&mut biff, 0x000A, &[]);
        push_record(&mut biff, 0x0809, &bof(0x0010));
        push_record(&mut biff, 0x0204, &label(0, 0, "H"));
        push_record(&mut biff, 0x0204, &label(0, 1, "I"));
        let mut mul = Vec::new();
        mul.extend_from_slice(&1u16.to_le_bytes());
        mul.extend_from_slice(&0u16.to_le_bytes());
        mul.extend_from_slice(&0u16.to_le_bytes());
        mul.extend_from_slice(&50u32.to_le_bytes());
        mul.extend_from_slice(&0u16.to_le_bytes());
        mul.extend_from_slice(&((3u32 << 2) | 2).to_le_bytes());
        mul.extend_from_slice(&1u16.to_le_bytes());
        push_record(&mut biff, 0x00BD, &mul);
        let mut formula = vec![0u8; 22];
        formula[0..2].copy_from_slice(&2u16.to_le_bytes());
        formula[6] = 0x00;
        formula[12] = 0xFF;
        formula[13] = 0xFF;
        push_record(&mut biff, 0x0006, &formula);
        push_record(&mut biff, 0x0207, &encode_xl("Ada"));
        push_record(&mut biff, 0x000A, &[]);
        let got = records(&compound(&biff)).unwrap();
        assert_eq!(got[0], vec!["H".to_string(), "I".to_string()]);
        assert_eq!(got[1], vec!["12".to_string(), "3".to_string()]);
        assert_eq!(got[2], vec!["Ada".to_string(), String::new()]);
    }

    #[test]
    fn a_stream_past_the_mini_cutoff_is_read_from_regular_sectors() {
        let long = "N".repeat(2200);
        let mut biff = Vec::new();
        push_record(&mut biff, 0x0809, &bof(0x0005));
        push_sst(&mut biff, std::slice::from_ref(&long), 1);
        push_record(&mut biff, 0x000A, &[]);
        push_record(&mut biff, 0x0809, &bof(0x0010));
        push_record(&mut biff, 0x00FD, &labelsst(0, 0, 0));
        push_record(&mut biff, 0x0204, &label(1, 0, "Ada"));
        push_record(&mut biff, 0x000A, &[]);
        assert!(biff.len() >= CUTOFF);
        let got = records(&compound(&biff)).unwrap();
        assert_eq!(got[0][0], long);
        assert_eq!(got[1][0], "Ada");
    }

    #[test]
    fn a_continue_record_between_shared_strings_is_joined() {
        let mut biff = Vec::new();
        push_record(&mut biff, 0x0809, &bof(0x0010));
        let mut first = Vec::new();
        first.extend_from_slice(&2u32.to_le_bytes());
        first.extend_from_slice(&2u32.to_le_bytes());
        first.extend_from_slice(&encode_xl("Name"));
        push_record(&mut biff, 0x00FC, &first);
        push_record(&mut biff, 0x003C, &encode_xl("Ada"));
        push_record(&mut biff, 0x00FD, &labelsst(0, 0, 0));
        push_record(&mut biff, 0x00FD, &labelsst(1, 0, 1));
        push_record(&mut biff, 0x000A, &[]);
        let got = records(&compound(&biff)).unwrap();
        assert_eq!(got[0][0], "Name");
        assert_eq!(got[1][0], "Ada");
    }

    #[test]
    fn bytes_that_are_not_a_workbook_say_so() {
        let err = records(b"Name\nAda\n").unwrap_err();
        assert!(err.to_string().contains("xls"), "{err}");
    }
}
