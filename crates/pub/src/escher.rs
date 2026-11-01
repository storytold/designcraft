// Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0
//! `Escher/EscherStm`: Office Drawing records ([MS-ODRAW]) for every page's shapes, and the
//! picture store whose pictures live in `Escher/EscherDelayStm`.
//!
//! Publisher's own variants: each DggContainer/DgContainer is followed by a u32, and the
//! client anchor/data records hold a duplicated u32 length then (u16 id, u32 value) pairs —
//! the anchor's are page-centre-relative EMU (`0x2001..0x2004`), the data's `0x6801` is the
//! shape's `Contents` sequence number.

use std::collections::HashMap;

use crate::read::{i32_at, slice, u16_at, u32_at};
use crate::{PubError, Result};

const DGG: u16 = 0xF000;
const BSTORE: u16 = 0xF001;
const DG: u16 = 0xF002;
const SPGR: u16 = 0xF003;
const SP: u16 = 0xF004;
const BSE: u16 = 0xF007;
const FSPGR: u16 = 0xF009;
const FSP: u16 = 0xF00A;
const FOPT: u16 = 0xF00B;
const CHILD_ANCHOR: u16 = 0xF00F;
const CLIENT_ANCHOR: u16 = 0xF010;
const CLIENT_DATA: u16 = 0xF011;
const TERTIARY_FOPT: u16 = 0xF122;

const MAX_DEPTH: usize = 16;

#[derive(Clone, Copy, Debug)]
struct Rec {
    inst: u16,
    ty: u16,
    data: usize,
    len: usize,
}

fn records(b: &[u8], start: usize, end: usize) -> Result<Vec<Rec>> {
    let end = end.min(b.len());
    let mut out = Vec::new();
    let mut at = start;
    while at + 8 <= end {
        let vi = u16_at(b, at)?;
        let ty = u16_at(b, at + 2)?;
        let len = u32_at(b, at + 4)? as usize;
        let data = at + 8;
        let stop = data.checked_add(len).ok_or(PubError::Truncated)?;
        if stop > end {
            return Err(PubError::Truncated);
        }
        out.push(Rec { inst: vi >> 4, ty, data, len });
        at = stop;
        if matches!(ty, DGG | DG) && at + 4 <= end {
            at += 4; // Publisher's trailing u32
        }
        if out.len() > 1_000_000 {
            return Err(PubError::Malformed("too many drawing records".into()));
        }
    }
    Ok(out)
}

/// An axis-aligned rectangle in EMU (page-centre-relative for page shapes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Emu {
    pub l: f64,
    pub t: f64,
    pub r: f64,
    pub b: f64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct EShape {
    pub seq: Option<u32>,
    pub kind: u16,
    pub flip_h: bool,
    pub flip_v: bool,
    pub anchor: Emu,
    pub props: HashMap<u16, u32>,
    pub complex: HashMap<u16, Vec<u8>>,
    /// Members of a group, already in page coordinates.
    pub children: Vec<EShape>,
    pub is_group: bool,
}

impl EShape {
    pub fn prop(&self, id: u16) -> Option<u32> {
        self.props.get(&id).copied()
    }
}

#[derive(Debug, Default)]
pub(crate) struct Drawing {
    /// Top-level shapes of every drawing, in stacking order (back to front).
    pub shapes: Vec<EShape>,
    /// Pictures by picture id (1-based `pib`).
    pub pictures: HashMap<u32, Picture>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Picture {
    pub kind: u16,
    pub data: Vec<u8>,
}

fn props(b: &[u8], r: Rec, sh: &mut EShape) -> Result<()> {
    let n = r.inst as usize;
    let mut complex_ids = Vec::new();
    for i in 0..n {
        let at = r.data + 6 * i;
        if at + 6 > r.data + r.len {
            break;
        }
        let id = u16_at(b, at)?;
        let v = u32_at(b, at + 2)?;
        let pid = id & 0x3fff;
        if pid & 0x3f == 0x3f {
            // Boolean sets (…BF, …FF) appear in both the primary and tertiary tables, each
            // setting only the flags whose "use" bit (value bit + 16) it carries: merge them.
            let old = sh.props.get(&pid).copied().unwrap_or(0);
            let used = v >> 16;
            sh.props.insert(pid, (old & !(used | used << 16)) | (v & (used | used << 16)));
        } else {
            sh.props.insert(pid, v);
        }
        if id & 0x8000 != 0 {
            complex_ids.push((pid, v as usize));
        }
    }
    let mut at = r.data + 6 * n;
    for (pid, len) in complex_ids {
        let end = (r.data + r.len).min(at.saturating_add(len));
        if let Ok(s) = slice(b, at, end.saturating_sub(at)) {
            sh.complex.insert(pid, s.to_vec());
        }
        at = end;
    }
    Ok(())
}

fn pairs(b: &[u8], r: Rec) -> Result<HashMap<u16, u32>> {
    let mut m = HashMap::new();
    let mut at = r.data + 4;
    while at + 6 <= r.data + r.len {
        m.insert(u16_at(b, at)?, u32_at(b, at + 2)?);
        at += 6;
    }
    Ok(m)
}

/// Map a child rectangle from a group's coordinate system to the group's frame.
fn map(rect: Emu, from: Emu, to: Emu) -> Emu {
    let sx = if (from.r - from.l).abs() > 0.0 { (to.r - to.l) / (from.r - from.l) } else { 1.0 };
    let sy = if (from.b - from.t).abs() > 0.0 { (to.b - to.t) / (from.b - from.t) } else { 1.0 };
    Emu { l: to.l + (rect.l - from.l) * sx, t: to.t + (rect.t - from.t) * sy, r: to.l + (rect.r - from.l) * sx, b: to.t + (rect.b - from.t) * sy }
}

fn shape(b: &[u8], sp: Rec) -> Result<(EShape, Option<Emu>)> {
    let mut sh = EShape::default();
    let mut coord_system = None;
    for r in records(b, sp.data, sp.data + sp.len)? {
        match r.ty {
            FSPGR => {
                coord_system = Some(Emu {
                    l: f64::from(i32_at(b, r.data)?),
                    t: f64::from(i32_at(b, r.data + 4)?),
                    r: f64::from(i32_at(b, r.data + 8)?),
                    b: f64::from(i32_at(b, r.data + 12)?),
                });
            }
            FSP => {
                sh.kind = r.inst;
                let flags = u32_at(b, r.data + 4)?;
                sh.is_group = flags & 0x1 != 0;
                sh.flip_h = flags & 0x40 != 0;
                sh.flip_v = flags & 0x80 != 0;
            }
            FOPT | TERTIARY_FOPT => props(b, r, &mut sh)?,
            CLIENT_ANCHOR => {
                let m = pairs(b, r)?;
                let v = |k| m.get(&k).map_or(0.0, |v| f64::from(*v as i32));
                sh.anchor = Emu { l: v(0x2001), t: v(0x2002), r: v(0x2003), b: v(0x2004) };
            }
            CHILD_ANCHOR => {
                sh.anchor = Emu {
                    l: f64::from(i32_at(b, r.data)?),
                    t: f64::from(i32_at(b, r.data + 4)?),
                    r: f64::from(i32_at(b, r.data + 8)?),
                    b: f64::from(i32_at(b, r.data + 12)?),
                };
            }
            CLIENT_DATA => sh.seq = pairs(b, r)?.get(&0x6801).copied(),
            _ => {}
        }
    }
    Ok((sh, coord_system))
}

/// A group container: its first shape describes the group (coordinate system and frame), the
/// rest are members. Members are mapped into the group's frame (page coordinates).
fn group(b: &[u8], spgr: Rec, depth: usize, warnings: &mut Vec<String>) -> Result<Option<EShape>> {
    if depth > MAX_DEPTH {
        warnings.push("Groups nested too deeply were skipped.".into());
        return Ok(None);
    }
    let recs = records(b, spgr.data, spgr.data + spgr.len)?;
    let mut leader: Option<(EShape, Option<Emu>)> = None;
    let mut members = Vec::new();
    for r in recs {
        match r.ty {
            SP if leader.is_none() => leader = Some(shape(b, r)?),
            SP => members.push(shape(b, r)?.0),
            SPGR => {
                if let Some(g) = group(b, r, depth + 1, warnings)? {
                    members.push(g);
                }
            }
            _ => {}
        }
    }
    let Some((mut head, cs)) = leader else { return Ok(None) };
    if let Some(cs) = cs {
        // Children's anchors are in the group's coordinate system; map them to its unrotated
        // frame (a group turned 45°–135° stores its frame with width and height swapped).
        let mut frame = head.anchor;
        let turn = head.prop(0x0004).map_or(0.0, |v| f64::from(v as i32) / 65_536.0).rem_euclid(180.0);
        if (45.0..135.0).contains(&turn) {
            let (cx, cy, hw, hh) = ((frame.l + frame.r) / 2.0, (frame.t + frame.b) / 2.0, (frame.r - frame.l) / 2.0, (frame.b - frame.t) / 2.0);
            frame = Emu { l: cx - hh, t: cy - hw, r: cx + hh, b: cy + hw };
        }
        let has_frame = frame != Emu::default();
        for m in &mut members {
            remap(m, cs, frame, has_frame);
        }
    }
    head.children = members;
    head.is_group = true;
    Ok(Some(head))
}

fn remap(sh: &mut EShape, from: Emu, to: Emu, apply: bool) {
    if apply {
        sh.anchor = map(sh.anchor, from, to);
        for c in &mut sh.children {
            remap(c, from, to, apply);
        }
    }
}

pub(crate) fn parse(stm: &[u8], delay: &[u8]) -> Result<Drawing> {
    let mut d = Drawing::default();
    for top in records(stm, 0, stm.len())? {
        match top.ty {
            DGG => {
                for r in records(stm, top.data, top.data + top.len)? {
                    if r.ty == BSTORE {
                        for (i, bse) in records(stm, r.data, r.data + r.len)?.iter().enumerate() {
                            if bse.ty != BSE {
                                continue;
                            }
                            let id = u32::try_from(i + 1).unwrap_or(u32::MAX);
                            match picture(stm, *bse, delay) {
                                Ok(Some(p)) => {
                                    d.pictures.insert(id, p);
                                }
                                Ok(None) => {}
                                Err(e) => d.warnings.push(format!("A picture could not be read ({e}).")),
                            }
                        }
                    }
                }
            }
            DG => {
                for r in records(stm, top.data, top.data + top.len)? {
                    if r.ty != SPGR {
                        continue;
                    }
                    let recs = records(stm, r.data, r.data + r.len)?;
                    // The first shape is the drawing's patriarch (the page group).
                    for (i, x) in recs.iter().enumerate() {
                        match x.ty {
                            SP if i == 0 => {}
                            SP => d.shapes.push(shape(stm, *x)?.0),
                            SPGR => {
                                if let Some(g) = group(stm, *x, 1, &mut d.warnings)? {
                                    d.shapes.push(g);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(d)
}

/// A BSE's picture: inline after the BSE header, or at `foDelay` in the delay stream.
fn picture(stm: &[u8], bse: Rec, delay: &[u8]) -> Result<Option<Picture>> {
    if bse.len < 36 {
        return Ok(None);
    }
    let size = u32_at(stm, bse.data + 20)? as usize;
    let fo_delay = u32_at(stm, bse.data + 28)? as usize;
    let cb_name = stm.get(bse.data + 33).copied().unwrap_or(0) as usize;
    if size == 0 {
        return Ok(None);
    }
    let inline = bse.data + 36 + cb_name;
    let (src, at) = if bse.len > 36 + cb_name + 8 { (stm, inline) } else { (delay, fo_delay) };
    let kind = u16_at(src, at + 2)?;
    let inst = u16_at(src, at)? >> 4;
    let len = u32_at(src, at + 4)? as usize;
    let body = slice(src, at + 8, len)?;
    // Header after the record header: one or two 16-byte UIDs, then (bitmaps) a tag byte or
    // (metafiles) a 34-byte metafile header, which is kept for the metafile reader.
    let two_uids = match kind {
        0xF01A => inst == 0x3D5,
        0xF01B => inst == 0x217,
        0xF01C => inst == 0x543,
        0xF01D => inst == 0x46B || inst == 0x6E3,
        0xF01E => inst == 0x6E1,
        0xF01F => inst == 0x7A9,
        0xF029 => inst == 0x6E5,
        0xF02A => inst == 0x6E3,
        _ => false,
    };
    let uids = if two_uids { 32 } else { 16 };
    let skip = match kind {
        0xF01A..=0xF01C => uids,
        _ => uids + 1,
    };
    let data = body.get(skip..).unwrap_or_default().to_vec();
    Ok(Some(Picture { kind, data }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(inst: u16, ver: u16, ty: u16, data: &[u8]) -> Vec<u8> {
        let mut v = ((inst << 4) | ver).to_le_bytes().to_vec();
        v.extend(ty.to_le_bytes());
        v.extend(u32::try_from(data.len()).unwrap().to_le_bytes());
        v.extend(data);
        v
    }

    fn prop(id: u16, value: u32) -> Vec<u8> {
        let mut v = id.to_le_bytes().to_vec();
        v.extend(value.to_le_bytes());
        v
    }

    #[test]
    fn tertiary_booleans_only_override_the_flags_they_use() {
        // Primary: line off (fUsefLine set, fLine clear). Tertiary: an unrelated flag's use bit.
        let mut body = rec(1, 3, FOPT, &prop(0x01ff, 0x0008_0000));
        body.extend(rec(1, 3, TERTIARY_FOPT, &prop(0x01ff, 0x0040_0000)));
        let sp = rec(0, 0xf, SP, &body);
        let (sh, _) = shape(&sp, Rec { inst: 0, ty: SP, data: 8, len: body.len() }).unwrap();
        let v = sh.prop(0x01ff).unwrap();
        assert_eq!(v & 0x0008_0008, 0x0008_0000, "line flag lost: {v:#x}");
        assert_ne!(v & 0x0040_0000, 0);
    }
}
