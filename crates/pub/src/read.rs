// Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0
//! Bounded little-endian and property-list decoding; see docs/pub-format/property-lists.md.
use crate::{PubError, Result};

pub(crate) const MAX_ITEMS: usize = 1_000_000;
pub(crate) fn slice(b: &[u8], at: usize, len: usize) -> Result<&[u8]> {
    let end = at.checked_add(len).ok_or(PubError::Truncated)?;
    b.get(at..end).ok_or(PubError::Truncated)
}
pub(crate) fn u16_at(b: &[u8], at: usize) -> Result<u16> {
    let bytes = slice(b, at, 2)?.try_into().map_err(|_| PubError::Truncated)?;
    Ok(u16::from_le_bytes(bytes))
}
pub(crate) fn u32_at(b: &[u8], at: usize) -> Result<u32> {
    let bytes = slice(b, at, 4)?.try_into().map_err(|_| PubError::Truncated)?;
    Ok(u32::from_le_bytes(bytes))
}
pub(crate) fn i32_at(b: &[u8], at: usize) -> Result<i32> {
    Ok(u32_at(b, at)? as i32)
}
pub(crate) fn count(n: u32, max: usize) -> Result<usize> {
    let n = n as usize;
    if n > max {
        return Err(PubError::Malformed("item count exceeds limit".into()));
    }
    Ok(n)
}
pub(crate) fn record(b: &[u8], at: usize) -> Result<&[u8]> {
    let len = u32_at(b, at)? as usize;
    if len < 4 {
        return Err(PubError::Malformed("list length below four".into()));
    }
    slice(b, at, len)?.get(4..).ok_or(PubError::Truncated)
}
#[derive(Clone, Copy)]
pub(crate) struct Prop<'a> {
    pub key: u8,
    pub kind: u8,
    pub data: &'a [u8],
}
impl Prop<'_> {
    pub fn number(self) -> Option<u32> {
        match self.data.len() {
            2 => u16_at(self.data, 0).ok().map(u32::from),
            4 => u32_at(self.data, 0).ok(),
            _ => None,
        }
    }
    pub fn children(self) -> Result<Vec<Self>> {
        properties(self.data)
    }
    pub fn text(self) -> String {
        let units: Vec<_> = self.data.as_chunks::<2>().0.iter().map(|p| u16::from_le_bytes([p[0], p[1]])).take_while(|u| *u != 0).collect();
        String::from_utf16_lossy(&units)
    }
}
pub(crate) fn properties(b: &[u8]) -> Result<Vec<Prop<'_>>> {
    let mut out = Vec::new();
    let mut at = 0;
    while b.len().saturating_sub(at) >= 2 {
        if out.len() >= MAX_ITEMS {
            return Err(PubError::Malformed("too many properties".into()));
        }
        let key = *b.get(at).ok_or(PubError::Truncated)?;
        let kind = *b.get(at + 1).ok_or(PubError::Truncated)?;
        at += 2;
        let len = match kind {
            0x00 | 0x02 | 0x03 | 0x04 | 0x05 | 0x08 | 0x0a | 0x0f | 0x78 => 0,
            0x07 | 0x10 | 0x12 | 0x18 | 0x1a => 2,
            0x20 | 0x22 | 0x58 | 0x68 | 0x70 | 0xb8 => 4,
            0x28 => 8,
            0x38 => 16,
            0x48 => 24,
            0x80 | 0x82 | 0x88 | 0x8a | 0x90 | 0x98 | 0xa0 | 0xc0 => {
                let raw = record(b, at)?;
                out.push(Prop { key, kind, data: raw });
                at += raw.len() + 4;
                continue;
            }
            _ => return Err(PubError::Malformed(format!("unknown property kind {kind:#x}"))),
        };
        out.push(Prop { key, kind, data: slice(b, at, len)? });
        at += len;
    }
    Ok(out)
}
pub(crate) fn number(props: &[Prop<'_>], key: u8) -> Option<u32> {
    props.iter().find(|p| p.key == key).and_then(|p| p.number())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_overflow_truncation_and_unknown_kinds() {
        assert!(slice(&[], usize::MAX, 2).is_err());
        assert!(u32_at(&[0; 3], 0).is_err());
        assert!(properties(&[1, 0xff]).is_err());
        assert!(properties(&[1, 0x88, 3, 0, 0, 0]).is_err());
        assert!(properties(&[1, 0x88, 8, 0, 0, 0]).is_err());
        assert!(count(u32::MAX, 100_000).is_err());
    }
    #[test]
    fn nested_lists_preserve_empty_slots_and_numeric_values() {
        let p = properties(&[0, 0x78, 1, 0x88, 8, 0, 0, 0, 2, 0x12, 42, 0]).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(number(&p[1].children().unwrap(), 2), Some(42));
    }
}
