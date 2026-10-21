//! The record index of a PageMaker 4–7 stream (PC or Mac).
//!
//! The header's index descriptor (offset 44) points either at the index itself, a run of 16-byte
//! entries, or at a list of 10-byte block references, each naming a run of entries. An entry
//! names a record type, a count and the offset of that many fixed-size records. A "group" entry
//! (type 1, flag 3) instead points at 10-byte chunk references, each holding part of a long
//! list of records of its subtype: large stories, many pages, embedded images.
//!
//! See `FORMAT.md` for the layout as observed in sample files.

use crate::bytes::Bytes;

/// One list of records of one type, in file order: its chunks as (count, offset).
#[derive(Clone, Debug, PartialEq)]
pub struct List {
    pub kind: u8,
    pub chunks: Vec<(usize, usize)>,
}

impl List {
    pub fn count(&self) -> usize {
        self.chunks.iter().map(|c| c.0).fold(0usize, usize::saturating_add)
    }

    /// The records, `size` bytes each; a record that would run past the end stops the list, and
    /// so does the source's read budget.
    pub fn records<'a>(&self, src: &Source<'a>, size: usize) -> Vec<&'a [u8]> {
        let size = size.max(1);
        let mut out = Vec::new();
        for &(count, at) in &self.chunks {
            // A count can't promise more records than the file holds after `at`.
            let fits = src.b.len().saturating_sub(at) / size;
            let n = src.take(count.min(fits).saturating_mul(size)) / size;
            for k in 0..n {
                match src.b.slice(at + k * size, size) {
                    Some(r) => out.push(r),
                    None => break,
                }
            }
        }
        out
    }

    /// The chunks' bytes joined (text and embedded files, one byte a record), at most `max`, and
    /// within the source's read budget.
    pub fn bytes(&self, src: &Source<'_>, max: usize) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        for &(count, at) in &self.chunks {
            let room = max.saturating_sub(out.len());
            let fits = src.b.len().saturating_sub(at);
            let n = src.take(count.min(fits).min(room));
            match src.b.slice(at, n) {
                Some(chunk) => out.extend_from_slice(chunk),
                None => break,
            }
            if out.len() >= max {
                break;
            }
        }
        out
    }
}

/// The file's bytes and how much more may be read from them. Chunk references come from the file
/// and may overlap or repeat, so without a budget a small hostile file could ask for gigabytes; every
/// read takes from one allowance tied to the file's size.
pub struct Source<'a> {
    pub b: Bytes<'a>,
    left: std::cell::Cell<usize>,
    /// A read was cut short by the budget.
    short: std::cell::Cell<bool>,
}

impl<'a> Source<'a> {
    /// Up to four times the file (real files read each byte about once), plus a little, at most 1 GiB.
    pub fn new(b: Bytes<'a>) -> Self {
        let allowance = b.len().saturating_mul(4).saturating_add(1 << 20).min(1 << 30);
        Source { b, left: std::cell::Cell::new(allowance), short: std::cell::Cell::new(false) }
    }

    /// Whether the budget ran out, so some records were left unread.
    pub fn exhausted(&self) -> bool {
        self.short.get()
    }

    /// Take up to `want` bytes of the budget; how many were granted.
    fn take(&self, want: usize) -> usize {
        let left = self.left.get();
        let granted = want.min(left);
        self.left.set(left - granted);
        if granted < want {
            self.short.set(true);
        }
        granted
    }
}

/// Index entry types, as observed.
pub mod kind {
    /// A group: its records are in chunks.
    pub const GROUP: u8 = 0x01;
    pub const PARA_RUNS: u8 = 0x0B;
    pub const TEXT: u8 = 0x0D;
    pub const IMAGE_DATA: u8 = 0x0E;
    pub const FONTS: u8 = 0x13;
    pub const COLORS: u8 = 0x15;
    pub const DOCUMENT: u8 = 0x18;
    pub const OBJECTS: u8 = 0x19;
    pub const STORY: u8 = 0x1B;
    pub const TRANSFORMS: u8 = 0x28;
    pub const CHAR_RUNS: u8 = 0x1C;
}

/// The most index entries read (real files have hundreds).
const MAX_ENTRIES: usize = 1 << 20;

/// The record lists of the index, in index order.
pub fn index(b: &Bytes<'_>) -> Vec<List> {
    let (Some(desc), Some(count), Some(at)) = (b.u8(45), b.u16(46), b.u32(48)) else { return vec![] };
    let count = usize::from(count);
    let at = at as usize;
    // Runs of 16-byte entries: the index itself, or the blocks a block list names.
    let runs: Vec<(usize, usize)> = match desc {
        0 => vec![(count, at)],
        _ => (0..count).filter_map(|k| Some((usize::from(b.u16(at + 10 * k + 2)?), b.u32(at + 10 * k + 4)? as usize))).collect(),
    };
    // Entries and chunk references are read from the file, and block lists may name the same
    // bytes again and again: no more of either than the file has room for.
    let max_entries = MAX_ENTRIES.min(b.len() / 16 + 64);
    let mut chunks_left = b.len() / 10 + 64;
    // Entries looked at, kept or not: a block list naming the same damaged block over and over
    // would otherwise spin through billions of them.
    let mut scanned = 0usize;
    let mut out = Vec::new();
    for (n, start) in runs {
        for k in 0..n {
            if out.len() >= max_entries || scanned >= max_entries {
                return out;
            }
            scanned += 1;
            let Some(e) = start.checked_add(k.saturating_mul(16)) else { break };
            let Some(entry) = b.slice(e, 16) else { break };
            // Every entry carries FF FF at 8; anything else is padding or damage.
            if entry.get(8..10) != Some(&[0xFF, 0xFF][..]) {
                continue;
            }
            let (Some(t), Some(c), Some(p), Some(flag), Some(sub)) = (b.u8(e + 1), b.u16(e + 2), b.u32(e + 4), b.u8(e + 10), b.u8(e + 11)) else {
                continue;
            };
            if t == kind::GROUP && flag == 3 {
                let take = usize::from(c).min(chunks_left);
                chunks_left -= take;
                let chunks = (0..take)
                    .filter_map(|j| {
                        let r = (p as usize).checked_add(10 * j)?;
                        (b.u8(r + 1)? == sub && b.slice(r + 8, 2)? == [0xFF, 0xFF]).then_some(())?;
                        Some((usize::from(b.u16(r + 2)?), b.u32(r + 4)? as usize))
                    })
                    .filter(|c| c.1 != 0)
                    .collect();
                out.push(List { kind: sub, chunks });
            } else {
                out.push(List { kind: t, chunks: if p == 0 { vec![] } else { vec![(usize::from(c), p as usize)] } });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One group whose 65 535 chunk references all name the same 64 KB: before the read budget,
    /// joining them asked for about 4 GB (and 68 GB of slices) and aborted the app.
    #[test]
    fn overlapping_chunks_stay_within_the_read_budget() {
        let refs_at = 128usize;
        let region = refs_at + 10 * 65_535;
        let mut f = vec![0u8; region + 65_535];
        f[6..8].copy_from_slice(&[0xFF, 0x99]);
        // Index descriptor: one 16-byte entry at 64.
        f[46..48].copy_from_slice(&1u16.to_le_bytes());
        f[48..52].copy_from_slice(&64u32.to_le_bytes());
        // A text group (subtype 0x0D) with 65 535 chunk references.
        let e = 64;
        f[e + 1] = kind::GROUP;
        f[e + 2..e + 4].copy_from_slice(&u16::MAX.to_le_bytes());
        f[e + 4..e + 8].copy_from_slice(&(refs_at as u32).to_le_bytes());
        f[e + 8..e + 10].copy_from_slice(&[0xFF, 0xFF]);
        f[e + 10] = 3;
        f[e + 11] = kind::TEXT;
        for j in 0..65_535 {
            let r = refs_at + 10 * j;
            f[r + 1] = kind::TEXT;
            f[r + 2..r + 4].copy_from_slice(&u16::MAX.to_le_bytes());
            f[r + 4..r + 8].copy_from_slice(&(region as u32).to_le_bytes());
            f[r + 8..r + 10].copy_from_slice(&[0xFF, 0xFF]);
        }
        let b = Bytes::new(&f, true);
        let lists = index(&b);
        assert_eq!(lists.len(), 1);
        assert_eq!(lists[0].chunks.len(), 65_535);
        let budget = f.len() * 4 + (1 << 20);
        let src = Source::new(b);
        let text = lists[0].bytes(&src, usize::MAX);
        assert!(text.len() <= budget, "{} bytes read from a {}-byte file", text.len(), f.len());
        // The budget is spent, and says so: nothing more comes out, whatever is asked.
        assert!(src.exhausted());
        assert!(lists[0].records(&src, 1).is_empty());
        assert!(lists[0].bytes(&src, usize::MAX).is_empty());
        // A fresh source, a cap: no more than asked, and the budget isn't what stopped it.
        let fresh = Source::new(b);
        assert_eq!(lists[0].bytes(&fresh, 1000).len(), 1000);
        assert!(!fresh.exhausted());
    }

    /// A block list naming one block of damaged entries 65 535 times, 65 535 entries each: before
    /// skipped entries counted, the index walked about 4.3 billion of them.
    #[test]
    fn damaged_entries_count_towards_the_scan_limit() {
        let blocks_at = 64usize;
        let entries_at = blocks_at + 10 * 65_535;
        let mut f = vec![0u8; entries_at + 16 * 64];
        f[6..8].copy_from_slice(&[0xFF, 0x99]);
        // Index descriptor: a block list of 65 535 references.
        f[45] = 1;
        f[46..48].copy_from_slice(&u16::MAX.to_le_bytes());
        f[48..52].copy_from_slice(&(blocks_at as u32).to_le_bytes());
        for k in 0..65_535 {
            let r = blocks_at + 10 * k;
            f[r + 2..r + 4].copy_from_slice(&u16::MAX.to_le_bytes());
            f[r + 4..r + 8].copy_from_slice(&(entries_at as u32).to_le_bytes());
        }
        let started = std::time::Instant::now();
        assert!(index(&Bytes::new(&f, true)).is_empty());
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "{:?}", started.elapsed());
    }
}
