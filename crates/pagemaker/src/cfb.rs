//! Just enough of a Compound File Binary (OLE) reader, after Microsoft's published [MS-CFB]
//! specification, to take one named stream out of a PageMaker 6–7 file: the header, the FAT (with
//! its DIFAT), the directory, and the mini stream for small streams.
//!
//! Every number comes from the file, so every read is checked: sector chains stop at a loop or at
//! the end of the file, and nothing is allocated beyond the file's own size.

use crate::bytes::Bytes;

/// The compound file signature.
pub const SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

const END_OF_CHAIN: u32 = 0xFFFF_FFFE;
/// Sector ids at or above this are markers (free, end of chain, FAT, DIFAT), never data.
const MAX_REGULAR: u32 = 0xFFFF_FFFA;
/// The largest stream taken out (PageMaker files are far smaller).
const MAX_STREAM: usize = 256 << 20;

pub fn is_cfb(data: &[u8]) -> bool {
    data.starts_with(&SIGNATURE)
}

struct Cfb<'a> {
    b: Bytes<'a>,
    sector: usize,
    mini: usize,
    fat: Vec<u32>,
}

impl<'a> Cfb<'a> {
    fn parse(data: &'a [u8]) -> Option<Self> {
        if !is_cfb(data) {
            return None;
        }
        let b = Bytes::new(data, true);
        let shift = b.u16(30)?;
        let mini_shift = b.u16(32)?;
        // Version 3 files use 512-byte sectors, version 4 files 4096; mini sectors are 64 bytes.
        if !(7..=12).contains(&shift) || !(2..=shift).contains(&mini_shift) {
            return None;
        }
        let sector = 1usize << shift;
        let fat_sectors = b.u32(44)? as usize;
        let mut difat: Vec<u32> = (0..109).filter_map(|i| b.u32(76 + 4 * i)).collect();
        // More FAT sectors are listed in a chain of DIFAT sectors.
        let mut next = b.u32(68)?;
        let mut seen = 0usize;
        let max_sectors = data.len() / sector + 1;
        while next < MAX_REGULAR && seen < max_sectors && difat.len() < fat_sectors {
            seen += 1;
            let at = Self::offset(sector, next)?;
            let per = sector / 4 - 1;
            difat.extend((0..per).filter_map(|i| b.u32(at + 4 * i)));
            next = b.u32(at + 4 * per)?;
        }
        let mut fat = Vec::new();
        for &s in difat.iter().take(fat_sectors.min(max_sectors)) {
            if s >= MAX_REGULAR {
                continue;
            }
            let at = Self::offset(sector, s)?;
            fat.extend((0..sector / 4).filter_map(|i| b.u32(at + 4 * i)));
        }
        Some(Cfb { b, sector, mini: 1usize << mini_shift, fat })
    }

    /// Byte offset of regular sector `s` (sector 0 follows the 512-byte header slot).
    fn offset(sector: usize, s: u32) -> Option<usize> {
        (s as usize).checked_add(1)?.checked_mul(sector)
    }

    /// The sector chain from `start`, stopping at the end, a marker or a loop.
    fn chain(&self, start: u32, table: &[u32]) -> Vec<u32> {
        let mut out = Vec::new();
        let mut seen = vec![false; table.len()];
        let mut s = start;
        while s < MAX_REGULAR {
            let Some(flag) = seen.get_mut(s as usize) else { break };
            if *flag {
                break;
            }
            *flag = true;
            out.push(s);
            s = table.get(s as usize).copied().unwrap_or(END_OF_CHAIN);
        }
        out
    }

    /// A stream in regular sectors, `len` bytes long (at most what its chain holds).
    fn read(&self, start: u32, len: usize) -> Vec<u8> {
        let mut out = Vec::new();
        for s in self.chain(start, &self.fat) {
            if out.len() >= len {
                break;
            }
            let Some(at) = Self::offset(self.sector, s) else { break };
            let Some(chunk) = self.b.slice(at, self.sector).or_else(|| self.b.data.get(at..)) else { break };
            out.extend_from_slice(chunk);
        }
        out.truncate(len);
        out
    }

    /// The stream named `wanted` (case-insensitive), if the directory has one.
    fn stream(&self, wanted: &str) -> Option<Vec<u8>> {
        let dir = self.read(self.b.u32(48)?, self.b.len());
        let d = Bytes::new(&dir, true);
        let entries = dir.len() / 128;
        let root = Entry::read(&d, 0)?;
        let cutoff = self.b.u32(56)? as usize;
        for i in 1..entries {
            let Some(e) = Entry::read(&d, i) else { continue };
            if e.kind != 2 || !e.name.eq_ignore_ascii_case(wanted) {
                continue;
            }
            let len = e.size.min(MAX_STREAM).min(self.b.len());
            if len >= cutoff {
                return Some(self.read(e.start, len));
            }
            // Small streams live in the mini stream, chained through the mini FAT.
            let mini_fat_bytes = self.read(self.b.u32(60)?, self.b.len());
            let mf = Bytes::new(&mini_fat_bytes, true);
            let mini_fat: Vec<u32> = (0..mini_fat_bytes.len() / 4).filter_map(|k| mf.u32(4 * k)).collect();
            let ministream = self.read(root.start, root.size.min(self.b.len()));
            let mut out = Vec::new();
            for s in self.chain(e.start, &mini_fat) {
                let at = (s as usize).checked_mul(self.mini)?;
                let Some(chunk) = ministream.get(at..at.saturating_add(self.mini).min(ministream.len())) else { break };
                out.extend_from_slice(chunk);
                if out.len() >= len {
                    break;
                }
            }
            out.truncate(len);
            return Some(out);
        }
        None
    }
}

struct Entry {
    name: String,
    kind: u8,
    start: u32,
    size: usize,
}

impl Entry {
    fn read(d: &Bytes<'_>, i: usize) -> Option<Entry> {
        let at = i.checked_mul(128)?;
        let name_len = usize::from(d.u16(at + 64)?).min(64);
        let units: Vec<u16> = (0..name_len.saturating_sub(2) / 2).filter_map(|k| d.u16(at + 2 * k)).collect();
        Some(Entry {
            name: String::from_utf16_lossy(&units),
            kind: d.u8(at + 66)?,
            start: d.u32(at + 116)?,
            // Version 3 files keep only the low 32 bits meaningful.
            size: d.u32(at + 120)? as usize,
        })
    }
}

/// The stream `name` of the compound file `data`, if it is one and has it.
pub fn stream(data: &[u8], name: &str) -> Option<Vec<u8>> {
    Cfb::parse(data)?.stream(name)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A version-3 compound file holding `streams` (each at least 4096 bytes, in regular
    /// sectors; smaller ones go in the mini stream).
    pub fn build(streams: &[(&str, &[u8])]) -> Vec<u8> {
        const SEC: usize = 512;
        let mut sectors: Vec<Vec<u8>> = Vec::new();
        let mut fat: Vec<u32> = Vec::new();
        let put = |data: &[u8], sectors: &mut Vec<Vec<u8>>, fat: &mut Vec<u32>| -> u32 {
            let first = sectors.len() as u32;
            let n = data.len().div_ceil(SEC).max(1);
            for k in 0..n {
                let mut s = data.get(k * SEC..((k + 1) * SEC).min(data.len())).unwrap_or_default().to_vec();
                s.resize(SEC, 0);
                sectors.push(s);
                fat.push(if k + 1 == n { END_OF_CHAIN } else { first + k as u32 + 1 });
            }
            first
        };
        // Mini stream for the small streams.
        let mut mini = Vec::new();
        let mut mini_fat: Vec<u32> = Vec::new();
        let mut placed: Vec<(u32, usize, bool)> = Vec::new();
        for (_, data) in streams {
            if data.len() < 4096 {
                let first = mini_fat.len() as u32;
                let n = data.len().div_ceil(64).max(1);
                for k in 0..n {
                    let mut s = data.get(k * 64..((k + 1) * 64).min(data.len())).unwrap_or_default().to_vec();
                    s.resize(64, 0);
                    mini.extend(s);
                    mini_fat.push(if k + 1 == n { END_OF_CHAIN } else { first + k as u32 + 1 });
                }
                placed.push((first, data.len(), true));
            } else {
                placed.push((u32::MAX, data.len(), false));
            }
        }
        for (k, (_, data)) in streams.iter().enumerate() {
            if !placed[k].2 {
                placed[k].0 = put(data, &mut sectors, &mut fat);
            }
        }
        let mini_start = if mini.is_empty() { END_OF_CHAIN } else { put(&mini, &mut sectors, &mut fat) };
        let mf_bytes: Vec<u8> = mini_fat.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mf_start = if mf_bytes.is_empty() { END_OF_CHAIN } else { put(&mf_bytes, &mut sectors, &mut fat) };
        // Directory: root, then the streams.
        let mut dir = Vec::new();
        let entry = |name: &str, kind: u8, start: u32, size: usize| {
            let mut e = vec![0u8; 128];
            let units: Vec<u16> = name.encode_utf16().chain([0]).collect();
            for (k, u) in units.iter().enumerate() {
                e[2 * k..2 * k + 2].copy_from_slice(&u.to_le_bytes());
            }
            e[64..66].copy_from_slice(&((units.len() * 2) as u16).to_le_bytes());
            e[66] = kind;
            e[68..72].copy_from_slice(&u32::MAX.to_le_bytes());
            e[72..76].copy_from_slice(&u32::MAX.to_le_bytes());
            e[76..80].copy_from_slice(&(if kind == 5 { 1u32 } else { u32::MAX }).to_le_bytes());
            e[116..120].copy_from_slice(&start.to_le_bytes());
            e[120..124].copy_from_slice(&(size as u32).to_le_bytes());
            e
        };
        dir.extend(entry("Root Entry", 5, mini_start, mini.len()));
        for (k, (name, _)) in streams.iter().enumerate() {
            dir.extend(entry(name, 2, placed[k].0, placed[k].1));
        }
        let dir_start = put(&dir, &mut sectors, &mut fat);
        // FAT sectors last; they mark themselves.
        let fat_sectors = (fat.len() + 1).div_ceil(SEC / 4 - 1).max(1);
        let fat_first = sectors.len() as u32;
        fat.extend(std::iter::repeat_n(0xFFFF_FFFD, fat_sectors));
        fat.resize(fat_sectors * SEC / 4, 0xFFFF_FFFF);
        for k in 0..fat_sectors {
            sectors.push(fat[k * SEC / 4..(k + 1) * SEC / 4].iter().flat_map(|v| v.to_le_bytes()).collect());
        }
        let mut h = vec![0u8; SEC];
        h[..8].copy_from_slice(&SIGNATURE);
        h[24..26].copy_from_slice(&0x3Eu16.to_le_bytes());
        h[26..28].copy_from_slice(&3u16.to_le_bytes());
        h[28..30].copy_from_slice(&0xFFFEu16.to_le_bytes());
        h[30..32].copy_from_slice(&9u16.to_le_bytes());
        h[32..34].copy_from_slice(&6u16.to_le_bytes());
        h[44..48].copy_from_slice(&(fat_sectors as u32).to_le_bytes());
        h[48..52].copy_from_slice(&dir_start.to_le_bytes());
        h[56..60].copy_from_slice(&4096u32.to_le_bytes());
        h[60..64].copy_from_slice(&mf_start.to_le_bytes());
        h[64..68].copy_from_slice(&(mini_fat.len().div_ceil(SEC / 4) as u32).to_le_bytes());
        h[68..72].copy_from_slice(&END_OF_CHAIN.to_le_bytes());
        for k in 0..109 {
            let v = if k < fat_sectors { fat_first + k as u32 } else { u32::MAX };
            h[76 + 4 * k..80 + 4 * k].copy_from_slice(&v.to_le_bytes());
        }
        let mut out = h;
        for s in sectors {
            out.extend(s);
        }
        out
    }

    #[test]
    fn takes_out_large_and_small_streams() {
        let big: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
        let small = b"tiny stream".to_vec();
        let file = build(&[("PageMaker", &big), ("Small", &small)]);
        assert_eq!(stream(&file, "PageMaker").unwrap(), big);
        assert_eq!(stream(&file, "pagemaker").unwrap(), big);
        assert_eq!(stream(&file, "Small").unwrap(), small);
        assert_eq!(stream(&file, "Missing"), None);
        assert_eq!(stream(b"not a compound file", "PageMaker"), None);
    }

    #[test]
    fn hostile_compound_files_never_panic() {
        let big: Vec<u8> = vec![7; 9000];
        let file = build(&[("PageMaker", &big)]);
        // Every truncation, and a loop in the FAT, read what they can and stop.
        for n in (0..file.len()).step_by(97) {
            let _ = stream(&file[..n], "PageMaker");
        }
        let mut looped = file.clone();
        // Point every FAT entry at sector 0.
        let fat_at = 512 * (1 + u32::from_le_bytes(looped[76..80].try_into().unwrap()) as usize);
        for k in 0..128 {
            looped[fat_at + 4 * k..fat_at + 4 * k + 4].copy_from_slice(&0u32.to_le_bytes());
        }
        let _ = stream(&looped, "PageMaker");
        let mut huge = file.clone();
        huge[30..32].copy_from_slice(&40u16.to_le_bytes());
        assert_eq!(stream(&huge, "PageMaker"), None);
        let mut sizes = file;
        for k in 0..4 {
            let dir = 512 * (1 + u32::from_le_bytes(sizes[48..52].try_into().unwrap()) as usize);
            sizes[dir + 128 * k + 120..dir + 128 * k + 124].copy_from_slice(&u32::MAX.to_le_bytes());
        }
        let _ = stream(&sizes, "PageMaker");
    }
}
