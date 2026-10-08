//! String decoding: 8-bit text is Windows-1252, wide text UTF-16LE.

const CP1252_HIGH: [u16; 32] = [
    0x20AC, 0xFFFD, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0xFFFD, 0x017D, 0xFFFD, 0xFFFD, 0x2018,
    0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0xFFFD, 0x017E, 0x0178,
];

pub(crate) fn cp1252(b: &[u8]) -> String {
    b.iter()
        .map(|&c| match c {
            0x80..=0x9F => CP1252_HIGH.get(usize::from(c - 0x80)).and_then(|&u| char::from_u32(u32::from(u))).unwrap_or('\u{FFFD}'),
            _ => char::from(c),
        })
        .collect()
}

pub(crate) fn utf16le(b: &[u8]) -> String {
    let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')).collect()
}

/// u16 count, u16 tag: tag & 0x4000 → 8-bit chars of length tag & 0x3FFF, else `count` UTF-16 units.
pub(crate) fn read_string(b: &[u8], off: usize) -> Option<(String, usize)> {
    let n = crate::bytes::u16_at(b, off)? as usize;
    let tag = crate::bytes::u16_at(b, off + 2)? as usize;
    if tag & 0x4000 != 0 {
        let len = tag & 0x3FFF;
        let s = crate::bytes::clamp(b, off + 4, len);
        Some((cp1252(s), off + 4 + len))
    } else {
        let s = crate::bytes::clamp(b, off + 4, n * 2);
        Some((utf16le(s), off + 4 + n * 2))
    }
}

pub(crate) fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

/// Number with up to six decimals, no trailing zeros, no negative zero.
pub(crate) fn num(x: f64) -> String {
    if !x.is_finite() {
        return "0".into();
    }
    let s = format!("{x:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".into() } else { s.to_string() }
}

pub(crate) fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk.first().copied().unwrap_or(0) as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;
        let n = (b0 << 16) | (b1 << 8) | b2;
        let ch = |i: usize| A.get((n >> (18 - 6 * i)) & 63).map(|&c| c as char).unwrap_or('A');
        out.push(ch(0));
        out.push(ch(1));
        out.push(if chunk.len() > 1 { ch(2) } else { '=' });
        out.push(if chunk.len() > 2 { ch(3) } else { '=' });
    }
    out
}
