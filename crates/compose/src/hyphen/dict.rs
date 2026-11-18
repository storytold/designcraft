//! Hyphenation exception dictionary: lowercase words with their break points.
//!
//! On-disk format (`assets/hyphenation/en-us.dic`): raw-deflate of
//! `"DCHYPH1\n"` followed by one line per word, sorted by the plain word's bytes. Each line is
//! front-coded against the previous one: a byte with the shared prefix length (in bytes of the
//! hyphenated form), then the rest of the hyphenated form (`-` at every break), then `\n`.

/// Sorted word list with a break bitmask per word (bit `i` = hyphen before char `i`).
#[derive(Clone, Debug, Default)]
pub struct Dictionary {
    words: String,
    offs: Vec<u32>,
    masks: Vec<u64>,
}

const MAGIC: &[u8] = b"DCHYPH1\n";

/// Most bytes a serialized dictionary may inflate to (the bundled English one is far smaller).
pub const MAX_INFLATED_BYTES: usize = 64 << 20;

impl Dictionary {
    /// Build from `(lowercase word, break mask)` pairs (sorted and de-duplicated here; the first
    /// entry of a duplicate wins).
    pub fn from_entries(mut entries: Vec<(String, u64)>) -> Dictionary {
        entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        entries.dedup_by(|b, a| a.0 == b.0);
        let mut d = Dictionary { words: String::new(), offs: vec![0], masks: Vec::with_capacity(entries.len()) };
        for (w, m) in entries {
            d.words.push_str(&w);
            d.offs.push(d.words.len() as u32);
            d.masks.push(m);
        }
        d
    }

    pub fn len(&self) -> usize {
        self.masks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.masks.is_empty()
    }

    fn word(&self, i: usize) -> &str {
        &self.words[self.offs[i] as usize..self.offs[i + 1] as usize]
    }

    /// Break mask of an exact (already lowercased) word.
    pub fn get(&self, word: &str) -> Option<u64> {
        let (mut lo, mut hi) = (0usize, self.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.word(mid).as_bytes().cmp(word.as_bytes()) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return Some(self.masks[mid]),
            }
        }
        None
    }

    /// Iterate `(word, mask)`.
    pub fn iter(&self) -> impl Iterator<Item = (&str, u64)> + '_ {
        (0..self.len()).map(|i| (self.word(i), self.masks[i]))
    }

    /// Serialize (deflate-compressed).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut raw = MAGIC.to_vec();
        let mut prev: Vec<u8> = Vec::new();
        for (w, m) in self.iter() {
            let h = hyphenated(w, m).into_bytes();
            let p = prev.iter().zip(&h).take_while(|(a, b)| a == b).count().min(255);
            raw.push(p as u8);
            raw.extend_from_slice(&h[p..]);
            raw.push(b'\n');
            prev = h;
        }
        miniz_oxide::deflate::compress_to_vec(&raw, 10)
    }

    /// Deserialize data written by [`Dictionary::to_bytes`]. At most [`MAX_INFLATED_BYTES`] are
    /// inflated: a few compressed bytes can stand for gigabytes.
    pub fn from_bytes(data: &[u8]) -> Result<Dictionary, String> {
        let raw = miniz_oxide::inflate::decompress_to_vec_with_limit(data, MAX_INFLATED_BYTES)
            .map_err(|e| format!("hyphenation dictionary: inflate failed (or over {} MB): {e:?}", MAX_INFLATED_BYTES >> 20))?;
        let body = raw.strip_prefix(MAGIC).ok_or("hyphenation dictionary: bad header")?;
        let mut d = Dictionary { words: String::with_capacity(body.len()), offs: vec![0], masks: Vec::new() };
        let mut cur: Vec<u8> = Vec::new();
        let mut i = 0;
        while i < body.len() {
            let p = body[i] as usize;
            let end = body[i + 1..].iter().position(|&b| b == b'\n').map(|e| i + 1 + e).ok_or("hyphenation dictionary: truncated")?;
            if p > cur.len() {
                return Err("hyphenation dictionary: bad prefix".into());
            }
            cur.truncate(p);
            cur.extend_from_slice(&body[i + 1..end]);
            i = end + 1;
            let s = std::str::from_utf8(&cur).map_err(|_| "hyphenation dictionary: bad utf-8")?;
            let (w, m) = parse_hyphenated(s);
            d.words.push_str(&w);
            d.offs.push(d.words.len() as u32);
            d.masks.push(m);
        }
        Ok(d)
    }
}

/// `word` with `-` before every char whose bit is set in `mask`.
pub fn hyphenated(word: &str, mask: u64) -> String {
    let mut s = String::with_capacity(word.len() + 8);
    for (i, c) in word.chars().enumerate() {
        if i > 0 && i < 64 && mask & (1 << i) != 0 {
            s.push('-');
        }
        s.push(c);
    }
    s
}

/// Inverse of [`hyphenated`].
pub fn parse_hyphenated(s: &str) -> (String, u64) {
    let mut w = String::with_capacity(s.len());
    let mut m = 0u64;
    let mut n = 0;
    for c in s.chars() {
        if c == '-' {
            if n > 0 && n < 64 {
                m |= 1 << n;
            }
        } else {
            w.push(c);
            n += 1;
        }
    }
    (w, m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let e: Vec<(String, u64)> = ["ty-pog-ra-phy", "cat", "hap-pen", "o'clock", "naïve-té", "naïve"].iter().map(|s| parse_hyphenated(s)).collect();
        let d = Dictionary::from_entries(e);
        let back = Dictionary::from_bytes(&d.to_bytes()).unwrap();
        assert_eq!(back.len(), 6);
        assert_eq!(back.get("typography").map(|m| hyphenated("typography", m)), Some("ty-pog-ra-phy".to_string()));
        assert_eq!(back.get("cat"), Some(0));
        assert_eq!(back.get("naïveté").map(|m| hyphenated("naïveté", m)), Some("naïve-té".to_string()));
        assert!(back.get("dog").is_none());
    }

    /// Data that inflates past the limit is refused instead of being inflated to the end.
    #[test]
    fn an_inflation_bomb_is_refused() {
        let mut raw = MAGIC.to_vec();
        raw.resize(MAX_INFLATED_BYTES + 1, b'a');
        let bomb = miniz_oxide::deflate::compress_to_vec(&raw, 10);
        assert!(bomb.len() < 1 << 20, "{}", bomb.len());
        let e = Dictionary::from_bytes(&bomb).err().unwrap_or_default();
        assert!(e.contains("inflate failed"), "{e}");
    }
}
