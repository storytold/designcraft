//! Object payloads: a sequence of `(u32 implementation id, u32 length, data)` blocks.

use std::collections::BTreeMap;

use crate::bytes::u32_at;
use crate::container::Container;
use crate::{Budget, InddError, Result};

#[derive(Debug, Clone)]
pub struct Obj {
    pub uid: u32,
    pub cls: u32,
    pub blocks: Vec<(u32, Vec<u8>)>,
    /// Blocks cover the payload exactly (false for raw data such as embedded files).
    pub ok: bool,
}

impl Obj {
    pub fn block(&self, implementation: u32) -> Option<&[u8]> {
        self.blocks.iter().find(|(i, _)| *i == implementation).map(|(_, d)| d.as_slice())
    }
}

pub fn parse_blocks(raw: &[u8]) -> (Vec<(u32, Vec<u8>)>, bool) {
    let mut blocks = Vec::new();
    let mut off = 0usize;
    while let (Some(imp), Some(len)) = (u32_at(raw, off), u32_at(raw, off + 4)) {
        let start = off + 8;
        let Some(data) = start.checked_add(len as usize).and_then(|end| raw.get(start..end)) else {
            return (blocks, false);
        };
        blocks.push((imp, data.to_vec()));
        off = start + data.len();
    }
    (blocks, off == raw.len())
}

/// All readable objects keyed by UID. Unreadable objects are skipped; the assembled bytes are
/// charged to `budget`, and running out of it is an error.
pub(crate) fn load(c: &Container<'_>, budget: &mut Budget) -> Result<BTreeMap<u32, Obj>> {
    let mut out = BTreeMap::new();
    for &uid in c.locations.keys() {
        let cls = c.classes.get(&uid).copied().unwrap_or(0);
        // Embedded files are kept out of the block model; the builder reads them on demand.
        if cls == crate::model::C_BLOB {
            out.insert(uid, Obj { uid, cls, blocks: Vec::new(), ok: false });
            continue;
        }
        let raw = match c.object_capped(uid, budget.left()) {
            Ok(raw) => raw,
            Err(e @ InddError::TooLarge(_)) => return Err(e),
            Err(_) => continue,
        };
        // The blocks are a copy of the raw bytes.
        budget.take(raw.len().saturating_mul(2), "objects")?;
        let (blocks, ok) = parse_blocks(&raw);
        out.insert(uid, Obj { uid, cls, blocks, ok });
    }
    Ok(out)
}
