//! Attribute lists. Graphic lists start with a u32 count, text lists with a u16 count; each entry is
//! `(u32 id, u16 len, typed values)` and typed values are `u16 n, n * (u32 type, u16 len, bytes)`.

use std::collections::BTreeMap;

use crate::bytes::{f64_at, u16_at, u32_at};

pub type Values = Vec<(u32, Vec<u8>)>;
pub type Attrs = BTreeMap<u32, Values>;

/// Value type of an integer (it is never a UID).
const T_INT: u32 = 0x6E67;

pub fn typed_values(data: &[u8]) -> Values {
    let Some(n) = u16_at(data, 0) else { return Vec::new() };
    let mut off = 2usize;
    let mut out = Vec::new();
    for _ in 0..n {
        let (Some(t), Some(len)) = (u32_at(data, off), u16_at(data, off + 4)) else { break };
        off += 6;
        out.push((t, crate::bytes::clamp(data, off, len as usize).to_vec()));
        off += len as usize;
    }
    out
}

fn entries(block: &[u8], mut off: usize, n: usize) -> Attrs {
    let mut out = BTreeMap::new();
    for _ in 0..n {
        let (Some(id), Some(len)) = (u32_at(block, off), u16_at(block, off + 4)) else { break };
        off += 6;
        out.insert(id, typed_values(crate::bytes::clamp(block, off, len as usize)));
        off += len as usize;
    }
    out
}

pub fn graphic_attrs(block: &[u8]) -> Attrs {
    u32_at(block, 0).map(|n| entries(block, 4, n as usize)).unwrap_or_default()
}

pub fn text_attrs(block: &[u8]) -> Attrs {
    u16_at(block, 0).map(|n| entries(block, 2, n as usize)).unwrap_or_default()
}

/// First 4-byte value that is not an integer, as a UID.
pub fn uid(values: Option<&Values>) -> Option<u32> {
    values?.iter().find(|(t, v)| v.len() == 4 && *t != T_INT).and_then(|(_, v)| u32_at(v, 0))
}

/// First 4-byte value of any type.
pub fn u32_value(values: Option<&Values>) -> Option<u32> {
    values?.iter().find(|(_, v)| v.len() == 4).and_then(|(_, v)| u32_at(v, 0))
}

pub fn double(values: Option<&Values>) -> Option<f64> {
    values?.iter().find(|(_, v)| v.len() == 8).and_then(|(_, v)| f64_at(v, 0))
}

/// First 2- or 4-byte value as an integer.
pub fn int(values: Option<&Values>) -> Option<i64> {
    let (_, v) = values?.iter().find(|(_, v)| v.len() == 2 || v.len() == 4)?;
    if v.len() == 2 { u16_at(v, 0).map(i64::from) } else { u32_at(v, 0).map(|x| i64::from(x as i32)) }
}

/// First value longer than 6 bytes read as a string at offset 3.
pub fn string(values: Option<&Values>) -> Option<String> {
    let (_, v) = values?.iter().find(|(_, v)| v.len() > 6)?;
    crate::text::read_string(v, 3).map(|(s, _)| s)
}
