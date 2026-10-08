//! The database container: 4 KiB pages, master pages, page map, UID B-trees and records.
//!
//! Every page ends with a 12-byte footer (u32 type, u32 link, u32 checksum). Master pages carry a
//! save sequence; the newest one points (through a shadow-paged root) at the page map, which maps
//! logical page numbers to physical pages. Type-6 pages are B-tree leaves for two trees: UID →
//! location and UID → class. Objects live as records on type-9 pages or span raw type-8 pages.

use std::collections::{BTreeMap, HashMap};

use crate::bytes::{clamp, u16_at, u32_at, u64_at};
use crate::{InddError, Result};

pub const PAGE: usize = 4096;
const FOOTER: usize = 12;
pub(crate) const GUID: [u8; 16] = [0x06, 0x06, 0xED, 0xF5, 0xD8, 0x1D, 0x46, 0xE5, 0xBD, 0x31, 0xEF, 0xE7, 0xFE, 0x74, 0xB7, 0x1D];
const NULL: u32 = 0xFFFF_FFFF;
const T_ROOT_B: u32 = 4;
const T_PAGEMAP: u32 = 5;
const T_BTREE: u32 = 6;
/// Upper bound for one assembled object.
const MAX_OBJECT: usize = 256 << 20;
const MAX_CHAIN: usize = 64;

/// Where a piece of an object is stored.
#[derive(Debug, Clone)]
pub struct Segment {
    /// Logical page when `slot != 0`, otherwise a physical raw page.
    pub page: u32,
    pub slot: u16,
    pub length: u16,
    pub index: u32,
}

pub struct Container<'a> {
    data: &'a [u8],
    pub seq: u64,
    pub major: u32,
    pub minor: u32,
    l2p: BTreeMap<u32, u32>,
    pub locations: BTreeMap<u32, Vec<Segment>>,
    pub classes: HashMap<u32, u32>,
}

impl<'a> Container<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        if !crate::is_indd(data) {
            return Err(InddError::NotIndd);
        }
        if data.get(24) != Some(&1) {
            return Err(InddError::Unsupported("big-endian document"));
        }
        let mut best: Option<(usize, u64)> = None;
        for i in 0..3 {
            let base = i * PAGE;
            if data.get(base..base + 16) != Some(&GUID[..]) {
                continue;
            }
            let seq = u64_at(data, base + 264).unwrap_or(0);
            if best.is_none_or(|(_, s)| seq > s) {
                best = Some((i, seq));
            }
        }
        let (bi, seq) = best.ok_or(InddError::NotIndd)?;
        let base = bi * PAGE;
        let major = u32_at(data, base + 29).unwrap_or(0);
        let minor = u32_at(data, base + 33).unwrap_or(0);
        let mut c = Container { data, seq, major, minor, l2p: BTreeMap::new(), locations: BTreeMap::new(), classes: HashMap::new() };
        let damaged = || InddError::Damaged("page map chain is broken".into());
        let root_b = u32_at(data, base + 0x3A8).ok_or_else(damaged)?;
        let pagemap = u32_at(data, (root_b as usize).saturating_mul(PAGE).saturating_add(128)).ok_or_else(damaged)?;
        if c.page_type(root_b) != Some(T_ROOT_B) || c.page_type(pagemap) != Some(T_PAGEMAP) {
            return Err(damaged());
        }
        let map = c.page(pagemap);
        for i in 0..(PAGE - FOOTER) / 4 - 32 {
            let Some(phys) = u32_at(map, (32 + i) * 4) else { break };
            let logical = i as u32;
            if phys != 0 && phys != NULL && (phys as usize).saturating_mul(PAGE) < data.len() && c.page_link(phys) == Some(logical) {
                c.l2p.insert(logical, phys);
            }
        }
        c.read_btrees();
        Ok(c)
    }

    /// Length of the whole file in bytes.
    pub fn file_len(&self) -> usize {
        self.data.len()
    }

    fn footer(&self, phys: u32, field: usize) -> Option<u32> {
        let off = (phys as usize).checked_mul(PAGE)?.checked_add(PAGE - FOOTER + field * 4)?;
        u32_at(self.data, off)
    }

    pub fn page_type(&self, phys: u32) -> Option<u32> {
        self.footer(phys, 0)
    }

    pub fn page_link(&self, phys: u32) -> Option<u32> {
        self.footer(phys, 1)
    }

    /// Page payload without the footer (empty when out of range).
    pub fn page(&self, phys: u32) -> &'a [u8] {
        clamp(self.data, (phys as usize).saturating_mul(PAGE), PAGE - FOOTER)
    }

    fn read_btrees(&mut self) {
        let leaves: Vec<u32> = self.l2p.values().copied().filter(|&p| self.page_type(p) == Some(T_BTREE)).collect();
        for phys in leaves {
            let p = self.page(phys);
            let Some(count) = u32_at(p, 0).map(|n| n as usize) else { continue };
            if count == 0 || count.saturating_mul(16).saturating_add(8) > p.len() {
                continue;
            }
            let entries: Vec<[u32; 4]> = (0..count)
                .filter_map(|i| {
                    let o = 8 + 16 * i;
                    Some([u32_at(p, o)?, u32_at(p, o + 4)?, u32_at(p, o + 8)?, u32_at(p, o + 12)?])
                })
                .collect();
            let class_tree = entries.iter().take(entries.len().saturating_sub(1)).all(|e| e[2] == 0 && e[3] == 0);
            for [uid, a, b, c] in entries {
                if class_tree {
                    if uid < 0x8000_0000 {
                        self.classes.insert(uid, a);
                    }
                } else {
                    let seg = Segment { page: b, slot: (a >> 16) as u16, length: (a & 0xFFFF) as u16, index: c };
                    self.locations.entry(uid).or_default().push(seg);
                }
            }
        }
    }

    /// Record body for `slot` on a logical page, following split-record continuations.
    pub fn record(&self, logical: u32, slot: u16) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let (mut logical, mut slot) = (logical, slot);
        for _ in 0..MAX_CHAIN {
            let phys = *self.l2p.get(&logical).ok_or_else(|| InddError::Damaged(format!("logical page {logical} is not mapped")))?;
            let p = self.page(phys);
            let mut off = 0usize;
            let mut found = None;
            while let (Some(ln), Some(s)) = (u16_at(p, off), u16_at(p, off + 2)) {
                let ln = ln as usize;
                if ln < 4 {
                    break;
                }
                if s & 0x7FFF == slot {
                    found = Some((clamp(p, off + 4, ln - 4), s & 0x8000 != 0));
                    break;
                }
                off += ln;
            }
            let (body, split) = found.ok_or_else(|| InddError::Damaged(format!("slot {slot} not found on logical page {logical}")))?;
            if !split {
                out.extend_from_slice(body);
                return Ok(out);
            }
            // Split record: u16 0, u16 next slot, u32 next logical page, then the first part.
            let next_slot = u16_at(body, 2).ok_or_else(|| InddError::Damaged("short split record".into()))?;
            let next_page = u32_at(body, 4).ok_or_else(|| InddError::Damaged("short split record".into()))?;
            out.extend_from_slice(body.get(8..).unwrap_or(&[]));
            if out.len() > MAX_OBJECT {
                return Err(InddError::Damaged("record too large".into()));
            }
            logical = next_page;
            slot = next_slot;
        }
        Err(InddError::Damaged("record continuation chain too long".into()))
    }

    /// Assembled object bytes. Segment 1 holds the tail; full raw pages (2..n) come first.
    pub fn object(&self, uid: u32) -> Result<Vec<u8>> {
        self.object_capped(uid, MAX_OBJECT)
    }

    /// Like [`Container::object`], failing with [`InddError::TooLarge`] once the object would
    /// exceed `cap` bytes (segments may repeat, so the size is not bounded by the file size).
    pub fn object_capped(&self, uid: u32, cap: usize) -> Result<Vec<u8>> {
        let mut segs = self.locations.get(&uid).cloned().ok_or_else(|| InddError::Damaged(format!("uid {uid} has no location")))?;
        segs.sort_by_key(|s| s.index);
        if !segs.is_empty() {
            segs.rotate_left(1);
        }
        let mut out = Vec::new();
        for s in segs {
            let len = s.length as usize;
            let record;
            let piece = if s.slot != 0 {
                record = self.record(s.page, s.slot)?;
                clamp(&record, 0, len)
            } else {
                clamp(self.page(s.page), 0, len)
            };
            let total = out.len().saturating_add(piece.len());
            if total > MAX_OBJECT {
                return Err(InddError::Damaged(format!("object {uid} too large")));
            }
            if total > cap {
                return Err(InddError::TooLarge("object data"));
            }
            out.extend_from_slice(piece);
        }
        Ok(out)
    }
}
