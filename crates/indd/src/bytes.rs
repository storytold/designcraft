//! Bounds-checked little-endian readers.

pub(crate) fn u16_at(b: &[u8], off: usize) -> Option<u16> {
    let s = b.get(off..off.checked_add(2)?)?;
    Some(u16::from_le_bytes([*s.first()?, *s.get(1)?]))
}

pub(crate) fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    let s = b.get(off..off.checked_add(4)?)?;
    let mut a = [0u8; 4];
    a.copy_from_slice(s);
    Some(u32::from_le_bytes(a))
}

pub(crate) fn u64_at(b: &[u8], off: usize) -> Option<u64> {
    let s = b.get(off..off.checked_add(8)?)?;
    let mut a = [0u8; 8];
    a.copy_from_slice(s);
    Some(u64::from_le_bytes(a))
}

pub(crate) fn f64_at(b: &[u8], off: usize) -> Option<f64> {
    u64_at(b, off).map(f64::from_bits)
}

pub(crate) fn slice(b: &[u8], off: usize, len: usize) -> Option<&[u8]> {
    b.get(off..off.checked_add(len)?)
}

/// Bytes from `off`, at most `len` of them (Python-style slice that never fails).
pub(crate) fn clamp(b: &[u8], off: usize, len: usize) -> &[u8] {
    let start = off.min(b.len());
    let end = off.saturating_add(len).min(b.len());
    b.get(start..end).unwrap_or(&[])
}

/// `count` u32 values starting at `off`, or None if they do not fit.
pub(crate) fn u32s(b: &[u8], off: usize, count: usize) -> Option<Vec<u32>> {
    let bytes = count.checked_mul(4)?;
    let s = slice(b, off, bytes)?;
    Some(s.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
}

/// u32 count at `off` followed by that many u32 values.
pub(crate) fn u32_list(b: &[u8], off: usize) -> Vec<u32> {
    u32_at(b, off).and_then(|n| u32s(b, off + 4, n as usize)).unwrap_or_default()
}

/// Six doubles (a b c d tx ty), or identity.
pub(crate) fn transform(b: Option<&[u8]>) -> [f64; 6] {
    let mut m = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    if let Some(b) = b
        && b.len() >= 48
    {
        for (i, v) in m.iter_mut().enumerate() {
            *v = f64_at(b, i * 8).unwrap_or(*v);
        }
    }
    m
}
