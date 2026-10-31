//! Liang hyphenation patterns (TeX notation, e.g. `.ab1c`, `4t1ion`): parsing and application.
//!
//! The bundled English set is *ours*: trained from the public-domain Moby Hyphenator word list by
//! [`super::patgen`] (see `examples/hyphgen.rs`). Patterns are stored in a hash map keyed by the
//! packed letter string, so applying them to a word costs `O(len × max_pattern_len)` lookups.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// Word-boundary marker in patterns.
pub const DOT: char = '.';
/// Longest pattern the packed key supports: 6 bits a letter below [`LEN_SHIFT`]. Installed pattern
/// files (hyph-utf8 and LibreOffice sets) have patterns of 10–11 letters, so 9 lost some.
pub const MAX_PATTERN_LEN: usize = 16;
/// Bit position of the letter count in a packed key.
pub const LEN_SHIFT: u32 = 120;

/// Small, fast multiplicative hasher for packed keys (FxHash-style).
#[derive(Default, Clone, Copy)]
pub struct FxHasher(u64);

impl Hasher for FxHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }
    fn write_u64(&mut self, i: u64) {
        self.0 = (self.0.rotate_left(5) ^ i).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    fn write_u128(&mut self, i: u128) {
        self.write_u64(i as u64);
        self.write_u64((i >> 64) as u64);
    }
}

pub type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;

/// Maps characters to 6-bit codes (1 = word boundary, 2.. = letters); 0 = not in the alphabet.
#[derive(Clone, Debug)]
pub struct Alphabet {
    ascii: [u8; 128],
    other: Vec<(char, u8)>,
    next: u8,
}

impl Default for Alphabet {
    fn default() -> Self {
        let mut a = Alphabet { ascii: [0; 128], other: Vec::new(), next: 2 };
        a.ascii[DOT as usize] = 1;
        a
    }
}

impl Alphabet {
    pub fn code(&self, c: char) -> u8 {
        if (c as u32) < 128 { self.ascii[c as usize] } else { self.other.iter().find(|x| x.0 == c).map_or(0, |x| x.1) }
    }
    /// Code for `c`, adding it if new (None when the 6-bit alphabet is full).
    pub fn intern(&mut self, c: char) -> Option<u8> {
        let k = self.code(c);
        if k != 0 {
            return Some(k);
        }
        if self.next >= 63 {
            return None;
        }
        let k = self.next;
        self.next += 1;
        if (c as u32) < 128 {
            self.ascii[c as usize] = k;
        } else {
            self.other.push((c, k));
        }
        Some(k)
    }
}

/// Packed key of a code string (≤ [`MAX_PATTERN_LEN`] codes).
#[inline]
pub fn pack(codes: &[u8]) -> u128 {
    let mut k = 0u128;
    for &c in codes {
        k = (k << 6) | c as u128;
    }
    k | ((codes.len() as u128) << LEN_SHIFT)
}

/// A compiled pattern set.
#[derive(Clone, Debug, Default)]
pub struct Patterns {
    alpha: Alphabet,
    /// Packed letters → inter-letter values (`len + 1` entries).
    map: FxMap<u128, Box<[u8]>>,
    max_len: usize,
}

/// ISO 8859-2 from 0xA0: LibreOffice's Serbian Latin patterns use it.
const ISO_8859_2: &str = "\u{A0}Ą˘Ł¤ĽŚ§¨ŠŞŤŹ\u{AD}ŽŻ°ą˛ł´ľśˇ¸šşťź˝žżŔÁÂĂÄĹĆÇČÉĘËĚÍÎĎĐŃŇÓÔŐÖ×ŘŮÚŰÜÝŢßŕáâăäĺćçčéęëěíîďđńňóôőö÷řůúűüýţ˙";

/// One byte of ISO 8859-5, which LibreOffice's Serbian Cyrillic patterns use: Cyrillic from 0xA1,
/// in Unicode's order, but for three signs.
fn iso_8859_5(b: u8) -> char {
    match b {
        0..=0xA0 | 0xAD => char::from(b),
        0xF0 => '№',
        0xFD => '§',
        _ => char::from_u32(0x0401 + u32::from(b - 0xA1)).unwrap_or('\u{FFFD}'),
    }
}

/// The text of a pattern file, in the charset its first line names: UTF-8, ISO8859-2 or ISO8859-5.
/// Other charsets (and bytes that aren't valid UTF-8 in a UTF-8 file) are None.
fn decode_hyphen_dic(file: &[u8]) -> Option<String> {
    let file = file.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(file);
    let first = file.split(|&b| b == b'\n').next()?;
    let charset = std::str::from_utf8(first).ok()?.trim().to_ascii_uppercase();
    match charset.as_str() {
        "UTF-8" => String::from_utf8(file.to_vec()).ok(),
        "ISO8859-2" | "ISO-8859-2" => {
            let high: Vec<char> = ISO_8859_2.chars().collect();
            file.iter().map(|&b| if b < 0xA0 { Some(char::from(b)) } else { high.get(usize::from(b) - 0xA0).copied() }).collect()
        }
        "ISO8859-5" | "ISO-8859-5" => Some(file.iter().map(|&b| iso_8859_5(b)).collect()),
        _ => None,
    }
}

impl Patterns {
    /// Parse TeX-style patterns: whitespace-separated, `%`/`#` start a comment line.
    pub fn parse(text: &str) -> Patterns {
        let mut p = Patterns::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('%') || line.starts_with('#') {
                continue;
            }
            for tok in line.split_whitespace() {
                p.insert(tok);
            }
        }
        p
    }

    /// Parse a LibreOffice/Hyphen pattern file (`hyph_sr_RS.dic`): a charset line ([`decode_hyphen_dic`]),
    /// then patterns as for [`Patterns::parse`]. The `LEFTHYPHENMIN`-style keyword lines
    /// are skipped (the paragraph's limits apply instead); of a two-level file (`NEXTLEVEL`) only
    /// the second level, the word patterns, is read; non-standard patterns (`/`) are skipped.
    /// Returns the patterns and how many were skipped, or None for another charset.
    pub fn parse_hyphen_dic(file: impl AsRef<[u8]>) -> Option<(Patterns, usize)> {
        let text = decode_hyphen_dic(file.as_ref())?;
        let body: Vec<&str> = text.lines().skip(1).collect();
        let start = body.iter().rposition(|l| l.trim() == "NEXTLEVEL").map_or(0, |i| i + 1);
        let mut p = Patterns::default();
        let mut skipped = 0;
        for line in body.get(start..).unwrap_or_default() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('%') || line.starts_with('#') || line.starts_with(|c: char| c.is_ascii_uppercase()) {
                continue;
            }
            for tok in line.split_whitespace() {
                if tok.contains('/') || !p.insert(&tok.to_lowercase()) {
                    skipped += 1;
                }
            }
        }
        Some((p, skipped))
    }

    /// Add one pattern like `a1b`; returns false if it can't be represented. A pattern with the
    /// same letters as one already there (`b2j` and `bj2`, as pattern files have) is merged with
    /// it, the higher value at each position, as applying both would give.
    pub fn insert(&mut self, pat: &str) -> bool {
        let mut codes = Vec::with_capacity(pat.len());
        let mut vals = vec![0u8];
        for c in pat.chars() {
            if let Some(d) = c.to_digit(10) {
                *vals.last_mut().unwrap_or(&mut 0) = d as u8;
            } else {
                let Some(k) = self.alpha.intern(c) else { return false };
                codes.push(k);
                vals.push(0);
            }
        }
        if codes.is_empty() || codes.len() > MAX_PATTERN_LEN {
            return false;
        }
        self.max_len = self.max_len.max(codes.len());
        let e = self.map.entry(pack(&codes)).or_insert_with(|| vec![0u8; vals.len()].into_boxed_slice());
        for (a, b) in e.iter_mut().zip(&vals) {
            *a = (*a).max(*b);
        }
        true
    }

    pub(crate) fn from_parts(alpha: Alphabet, map: FxMap<u128, Box<[u8]>>, max_len: usize) -> Patterns {
        Patterns { alpha, map, max_len }
    }

    /// Make `letters` part of the alphabet even if no pattern mentions them.
    pub fn add_letters(&mut self, letters: impl IntoIterator<Item = char>) {
        for c in letters {
            let _ = self.alpha.intern(c);
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Inter-letter values for a lowercase word: `out[i]` is the value *before* char `i`
    /// (`out.len() == word.len() + 1`). None if a char is outside the pattern alphabet.
    pub fn values(&self, word: &[char]) -> Option<Vec<u8>> {
        let mut codes = Vec::with_capacity(word.len() + 2);
        codes.push(1);
        for &c in word {
            let k = self.alpha.code(c);
            if k == 0 {
                return None;
            }
            codes.push(k);
        }
        codes.push(1);
        let mut vals = vec![0u8; codes.len() + 1];
        apply(&self.map, self.max_len, &codes, &mut vals);
        // Padded position p sits before padded char p; word char i is padded char i + 1.
        Some(vals[1..codes.len()].to_vec())
    }

    /// Break points (char indices, a hyphen goes before that char) of a lowercase word.
    pub fn points(&self, word: &[char]) -> Option<Vec<usize>> {
        let v = self.values(word)?;
        Some((1..word.len()).filter(|&i| v[i] % 2 == 1).collect())
    }

    /// The patterns in TeX notation (sorted by letters), e.g. for writing a pattern file.
    pub fn to_tex(&self) -> Vec<String> {
        let mut rev: Vec<(u8, char)> =
            (0u8..128).filter(|&c| self.alpha.ascii[c as usize] != 0).map(|c| (self.alpha.ascii[c as usize], c as char)).collect();
        rev.extend(self.alpha.other.iter().map(|&(c, k)| (k, c)));
        let ch = |k: u8| rev.iter().find(|x| x.0 == k).map_or('?', |x| x.1);
        let mut out: Vec<(String, String)> = self
            .map
            .iter()
            .map(|(&key, vals)| {
                let n = (key >> LEN_SHIFT) as usize;
                let letters: Vec<char> = (0..n).map(|i| ch(((key >> (6 * (n - 1 - i))) & 63) as u8)).collect();
                let mut s = String::new();
                for (i, l) in letters.iter().enumerate() {
                    if vals[i] > 0 {
                        s.push(char::from(b'0' + vals[i]));
                    }
                    s.push(*l);
                }
                if vals[n] > 0 {
                    s.push(char::from(b'0' + vals[n]));
                }
                (letters.into_iter().collect(), s)
            })
            .collect();
        out.sort();
        out.into_iter().map(|x| x.1).collect()
    }
}

/// Apply `map` to padded `codes`, raising `vals` (len = codes.len() + 1).
#[inline]
pub fn apply(map: &FxMap<u128, Box<[u8]>>, max_len: usize, codes: &[u8], vals: &mut [u8]) {
    let n = codes.len();
    for s in 0..n {
        let mut k = 0u128;
        for len in 1..=max_len.min(n - s) {
            k = (k << 6) | codes[s + len - 1] as u128;
            if let Some(v) = map.get(&(k | ((len as u128) << LEN_SHIFT))) {
                for (j, &x) in v.iter().enumerate() {
                    if x > vals[s + j] {
                        vals[s + j] = x;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liang_example() {
        // The classic example from Liang's thesis: "hy-phen-ation".
        let p = Patterns::parse("hy3ph he2n hena4 hen5at 1na n2at 1tio 2io o2n");
        let w: Vec<char> = "hyphenation".chars().collect();
        assert_eq!(p.points(&w).unwrap(), vec![2, 6]);
        let mut tex = p.to_tex();
        tex.sort();
        assert!(tex.contains(&"hen5at".to_string()));
        assert!(p.values(&['h', 'é']).is_none());
    }

    #[test]
    fn long_patterns_are_kept() {
        // 11 letters: over the old 9-letter limit, as in the Serbian pattern files.
        let mut p = Patterns::default();
        assert!(p.insert("1abcdefghi1jk"));
        let w: Vec<char> = "abcdefghijk".chars().collect();
        assert_eq!(p.points(&w).unwrap(), vec![9]);
        assert_eq!(p.to_tex(), vec!["1abcdefghi1jk".to_string()]);
        assert!(!p.insert(&"a".repeat(MAX_PATTERN_LEN + 1)));
    }

    #[test]
    fn same_letters_merge() {
        // Both sides of the pair: neither pattern may replace the other.
        let p = Patterns::parse("b2j bj2 1b");
        assert_eq!(p.to_tex(), vec!["1b".to_string(), "b2j2".to_string()]);
        let w: Vec<char> = "bjbj".chars().collect();
        assert_eq!(p.values(&w).unwrap(), vec![1, 2, 2, 2, 2]);
    }

    #[test]
    fn hyphen_dic_files() {
        let (p, skipped) = Patterns::parse_hyphen_dic("UTF-8\nLEFTHYPHENMIN 2\nRIGHTHYPHENMIN 2\n# comment\n1на\n1ти\nc1k/k=k,1,2\n").unwrap();
        assert_eq!(p.len(), 2, "the keyword lines are no patterns");
        assert_eq!(skipped, 1, "the non-standard pattern");
        let w: Vec<char> = "натина".chars().collect();
        assert_eq!(p.points(&w).unwrap(), vec![2, 4]);
        // Two levels: the second holds the word patterns.
        let (p, _) = Patterns::parse_hyphen_dic("UTF-8\n1x\nNEXTLEVEL\n1b\n").unwrap();
        assert_eq!(p.to_tex(), vec!["1b".to_string()]);
        assert!(Patterns::parse_hyphen_dic("KOI8-R\n1a\n").is_none(), "a charset we can't read");
        assert!(Patterns::parse_hyphen_dic(b"UTF-8\n1\xFF\n").is_none(), "not UTF-8 after all");
        assert!(Patterns::parse_hyphen_dic("").is_none());
    }

    /// LibreOffice's Serbian files name these charsets on their first line.
    #[test]
    fn pattern_files_in_iso_8859() {
        assert_eq!(decode_hyphen_dic(b"ISO8859-5\n\xBD\xD8\xF8\xF9\xFA\xFB\xFF\n").unwrap(), "ISO8859-5\nНијљњћџ\n");
        assert_eq!(decode_hyphen_dic(b"ISO8859-2\n\xC8\xE8\xC6\xE6\xD0\xF0\xA9\xB9\xAE\xBE\n").unwrap(), "ISO8859-2\nČčĆćĐđŠšŽž\n");
        assert_eq!(ISO_8859_2.chars().count(), 0x60, "every byte from 0xA0");
        let (p, _) = Patterns::parse_hyphen_dic(b"ISO8859-2\na1\xE8\n").unwrap();
        assert_eq!(p.points(&['a', 'č']), Some(vec![1]));
        let (p, _) = Patterns::parse_hyphen_dic(b"ISO8859-5\n\xD01\xDD\n").unwrap();
        assert_eq!(p.points(&['а', 'н']), Some(vec![1]));
    }
}
