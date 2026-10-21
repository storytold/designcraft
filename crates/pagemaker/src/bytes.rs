//! Bounds-checked reads in the file's byte order, and the 8-bit text encodings PageMaker writes
//! (Windows-1252 on the PC, Mac OS Roman on the Mac).

/// A byte slice read in one byte order. Every read is checked: past the end it returns `None`.
#[derive(Clone, Copy, Debug)]
pub struct Bytes<'a> {
    pub data: &'a [u8],
    /// Little-endian (PC); big-endian on the Mac.
    pub le: bool,
}

impl<'a> Bytes<'a> {
    pub fn new(data: &'a [u8], le: bool) -> Self {
        Bytes { data, le }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn u8(&self, at: usize) -> Option<u8> {
        self.data.get(at).copied()
    }

    pub fn u16(&self, at: usize) -> Option<u16> {
        let b = self.data.get(at..at.checked_add(2)?)?;
        let a = [*b.first()?, *b.get(1)?];
        Some(if self.le { u16::from_le_bytes(a) } else { u16::from_be_bytes(a) })
    }

    pub fn i16(&self, at: usize) -> Option<i16> {
        self.u16(at).map(|v| v as i16)
    }

    pub fn u32(&self, at: usize) -> Option<u32> {
        let b = self.data.get(at..at.checked_add(4)?)?;
        let a = [*b.first()?, *b.get(1)?, *b.get(2)?, *b.get(3)?];
        Some(if self.le { u32::from_le_bytes(a) } else { u32::from_be_bytes(a) })
    }

    /// `n` bytes from `at`, or `None` when they run past the end.
    pub fn slice(&self, at: usize, n: usize) -> Option<&'a [u8]> {
        self.data.get(at..at.checked_add(n)?)
    }
}

/// Windows-1252, 0x80..=0x9F (the rest is Latin-1).
const CP1252_HIGH: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜',
    '™', 'š', '›', 'œ', '\u{9D}', 'ž', 'Ÿ',
];

/// Mac OS Roman, 0x80..=0xFF.
const MAC_ROMAN_HIGH: &str =
    "ÄÅÇÉÑÖÜáàâäãåçéèêëíìîïñóòôöõúùûü†°¢£§•¶ß®©™´¨≠ÆØ∞±≤≥¥µ∂∑∏π∫ªºΩæø¿¡¬√ƒ≈∆«»…\u{A0}ÀÃÕŒœ–—“”‘’÷◊ÿŸ⁄€‹›ﬁﬂ‡·‚„‰ÂÊÁËÈÍÎÏÌÓÔ\u{F8FF}ÒÚÛÙıˆ˜¯˘˙˚¸˝˛ˇ";

/// One byte of text as a char: Windows-1252 on the PC, Mac OS Roman on the Mac.
pub fn char_of(b: u8, mac: bool) -> char {
    match (b, mac) {
        (0..=0x7F, _) => b as char,
        (_, true) => MAC_ROMAN_HIGH.chars().nth(usize::from(b - 0x80)).unwrap_or('\u{FFFD}'),
        (0x80..=0x9F, false) => CP1252_HIGH.get(usize::from(b - 0x80)).copied().unwrap_or('\u{FFFD}'),
        _ => b as char,
    }
}

/// A NUL-terminated (or full-width) name: font, style or colour names in fixed-size records.
pub fn name(bytes: &[u8], mac: bool) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    bytes.get(..end).unwrap_or_default().iter().map(|b| char_of(*b, mac)).collect::<String>().trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_both_byte_orders_and_stops_at_the_end() {
        let pc = Bytes::new(&[0x34, 0x12, 0x78, 0x56], true);
        assert_eq!(pc.u16(0), Some(0x1234));
        assert_eq!(pc.u32(0), Some(0x5678_1234));
        let mac = Bytes::new(&[0x12, 0x34, 0x56, 0x78], false);
        assert_eq!(mac.u16(0), Some(0x1234));
        assert_eq!(mac.u32(0), Some(0x1234_5678));
        assert_eq!(pc.u16(3), None);
        assert_eq!(pc.u32(usize::MAX), None);
        assert_eq!(pc.slice(2, usize::MAX), None);
    }

    #[test]
    fn decodes_both_code_pages() {
        assert_eq!(char_of(0x80, false), '€');
        assert_eq!(char_of(0xE9, false), 'é');
        assert_eq!(char_of(0x8E, true), 'é');
        assert_eq!(char_of(0xA5, true), '•');
        assert_eq!(MAC_ROMAN_HIGH.chars().count(), 128);
        assert_eq!(name(b"Times\0garbage", false), "Times");
    }
}
