//! CFF font sets (`StartData`), CIDFonts, CMaps (`CIDInit`) and composite (Type 0) fonts.

use std::rc::Rc;

use crate::gstate::M;
use crate::interp::{Interp, R, err, matrix_obj};
use crate::obj::{Key, Obj, PArr, PDict, PStr, name};
use crate::type1::Glyph;

/// A CMap: code space ranges, code → CID ranges (single codes are ranges of one), a parent.
#[derive(Debug, Default)]
pub struct CMap {
    codespace: Vec<(Vec<u8>, Vec<u8>)>,
    ranges: Vec<(Vec<u8>, Vec<u8>, u32)>,
    parent: Option<usize>,
    identity: bool,
    /// Vertical writing (WMode 1).
    pub vertical: bool,
}

const MAX_CMAPS: usize = 10_000;
const MAX_RANGES: usize = 500_000;

fn code_value(b: &[u8]) -> u64 {
    b.iter().fold(0u64, |a, x| a << 8 | u64::from(*x))
}

fn in_range(code: &[u8], lo: &[u8], hi: &[u8]) -> bool {
    code.len() == lo.len() && code.len() == hi.len() && code.iter().zip(lo).zip(hi).all(|((c, l), h)| c >= l && c <= h)
}

impl Interp {
    fn cmap_of(&self, d: &PDict) -> Option<usize> {
        match d.get_name(b"DesignCraftCMap") {
            Some(Obj::Int(i)) => usize::try_from(i).ok(),
            _ => None,
        }
    }

    /// The CMap the current dictionary is building (made on first use).
    fn building(&mut self) -> R<usize> {
        let d = self.current_dict()?;
        if let Some(i) = self.cmap_of(&d) {
            return Ok(i);
        }
        if self.cmaps.len() >= MAX_CMAPS {
            return err("limitcheck");
        }
        self.cmaps.push(CMap::default());
        let i = self.cmaps.len() - 1;
        d.put_name(b"DesignCraftCMap", Obj::Int(i as i64));
        Ok(i)
    }

    /// CID for a code through CMap `i` (and its parents); None when unmapped.
    fn cid(&self, i: usize, code: &[u8], depth: usize) -> Option<u32> {
        let m = self.cmaps.get(i)?;
        if m.identity {
            return u32::try_from(code_value(code)).ok();
        }
        for (lo, hi, start) in &m.ranges {
            if in_range(code, lo, hi) {
                return u32::try_from(u64::from(*start) + code_value(code).saturating_sub(code_value(lo))).ok();
            }
        }
        if depth < 8 { m.parent.and_then(|p| self.cid(p, code, depth + 1)) } else { None }
    }

    /// How many ranges a lookup through CMap `i` may compare.
    fn cmap_cost(&self, i: usize) -> u64 {
        let mut n = 0u64;
        let mut cur = Some(i);
        for _ in 0..8 {
            let Some(m) = cur.and_then(|c| self.cmaps.get(c)) else { break };
            n = n.saturating_add((m.ranges.len() + m.codespace.len()) as u64);
            cur = m.parent;
        }
        n
    }

    fn code_len(&self, i: usize, s: &[u8], depth: usize) -> Option<usize> {
        let m = self.cmaps.get(i)?;
        for n in 1..=4 {
            let Some(code) = s.get(..n) else { break };
            if m.codespace.iter().any(|(lo, hi)| in_range(code, lo, hi)) {
                return Some(n);
            }
        }
        if depth < 8 { m.parent.and_then(|p| self.code_len(p, s, depth + 1)) } else { None }
    }
}

// ----- CIDInit procedures -----

pub fn begincmap(_ip: &mut Interp) -> R {
    Ok(())
}

pub fn endcmap(ip: &mut Interp) -> R {
    let i = ip.building()?;
    let vertical = ip.current_dict()?.get_name(b"WMode").and_then(|o| o.num()) == Some(1.0);
    if let Some(m) = ip.cmaps.get_mut(i) {
        m.vertical = vertical;
    }
    Ok(())
}

pub fn usecmap(ip: &mut Interp) -> R {
    let n = ip.pop()?;
    let parent = find_cmap(ip, &n)?;
    let i = ip.building()?;
    if let Some(m) = ip.cmaps.get_mut(i) {
        m.parent = Some(parent);
    }
    Ok(())
}

/// `n begin…` pushes a mark for its entries.
pub fn begin_block(ip: &mut Interp) -> R {
    ip.pop_int()?;
    ip.push(Obj::Mark)
}

fn block(ip: &mut Interp) -> R<Vec<Obj>> {
    let i = crate::ops::basic::mark_pos(ip)?;
    let items = ip.o.split_off(i + 1);
    ip.o.pop();
    Ok(items)
}

fn bytes_of(o: &Obj) -> R<Vec<u8>> {
    match o {
        Obj::Str(s, _) => Ok(s.bytes()),
        _ => err("typecheck"),
    }
}

pub fn endcodespacerange(ip: &mut Interp) -> R {
    let items = block(ip)?;
    let i = ip.building()?;
    for p in items.chunks(2) {
        if let [lo, hi] = p {
            let (lo, hi) = (bytes_of(lo)?, bytes_of(hi)?);
            if let Some(m) = ip.cmaps.get_mut(i) {
                m.codespace.push((lo, hi));
            }
        }
    }
    Ok(())
}

fn add_ranges(ip: &mut Interp, triples: bool) -> R {
    let items = block(ip)?;
    let i = ip.building()?;
    let step = if triples { 3 } else { 2 };
    for e in items.chunks(step) {
        let (lo, hi, cid) = match (triples, e) {
            (true, [lo, hi, cid]) => (bytes_of(lo)?, bytes_of(hi)?, cid),
            (false, [code, cid]) => (bytes_of(code)?, bytes_of(code)?, cid),
            _ => continue,
        };
        let Some(cid) = cid.num() else { continue };
        if let Some(m) = ip.cmaps.get_mut(i)
            && m.ranges.len() < MAX_RANGES
        {
            m.ranges.push((lo, hi, cid.max(0.0) as u32));
        }
    }
    Ok(())
}

pub fn endcidrange(ip: &mut Interp) -> R {
    add_ranges(ip, true)
}

pub fn endcidchar(ip: &mut Interp) -> R {
    add_ranges(ip, false)
}

/// bf (Unicode) and notdef blocks: read and dropped.
pub fn end_ignored(ip: &mut Interp) -> R {
    block(ip).map(|_| ())
}

fn identity(ip: &mut Interp, n: &[u8]) -> PDict {
    let d = PDict::new(4);
    d.put_name(b"CMapName", Obj::lit(n));
    let vertical = n.ends_with(b"-V");
    ip.cmaps.push(CMap { codespace: vec![(vec![0, 0], vec![0xff, 0xff])], identity: true, vertical, ..Default::default() });
    d.put_name(b"DesignCraftCMap", Obj::Int(ip.cmaps.len() as i64 - 1));
    d.put_name(b"WMode", Obj::Int(i64::from(n.ends_with(b"-V"))));
    d
}

/// A CMap by name or dictionary: as an index into `ip.cmaps`.
fn find_cmap(ip: &mut Interp, o: &Obj) -> R<usize> {
    let d = match o {
        Obj::Dict(d) => d.clone(),
        n => {
            let cats = category(ip, b"CMap");
            match cats.get(&n.key()) {
                Some(Obj::Dict(d)) => d,
                _ => {
                    // Unknown CMaps read two-byte codes as CIDs, as Identity does.
                    let d = identity(ip, &crate::ops::basic::text_of(n));
                    cats.put(n.clone(), Obj::Dict(d.clone()));
                    d
                }
            }
        }
    };
    match ip.cmap_of(&d) {
        Some(i) => Ok(i),
        None => {
            let i = ip.cmaps.len();
            ip.cmaps.push(CMap { identity: true, codespace: vec![(vec![0, 0], vec![0xff, 0xff])], ..Default::default() });
            d.put_name(b"DesignCraftCMap", Obj::Int(i as i64));
            Ok(i)
        }
    }
}

fn category(ip: &mut Interp, cat: &[u8]) -> PDict {
    match ip.resources.get_name(cat) {
        Some(Obj::Dict(d)) => d,
        _ => {
            let d = PDict::new(20);
            ip.resources.put_name(cat, Obj::Dict(d.clone()));
            d
        }
    }
}

fn add_fid(ip: &mut Interp, d: &PDict) {
    if !d.known(&Key::Name(name(b"FID"))) {
        d.put_name(b"FID", Obj::FontId(ip.next_fid));
        ip.next_fid += 1;
    }
}

// ----- StartData -----

fn nums(v: &[f64]) -> Obj {
    Obj::Array(PArr::new(v.iter().map(|x| Obj::Real(*x)).collect()), false)
}

/// `/name length StartData`: the CFF font set in the next `length` bytes of the current file;
/// its fonts become Font (name-keyed) or CIDFont (CID-keyed) resources.
pub fn startdata(ip: &mut Interp) -> R {
    let len = ip.pop_usize()?;
    let _set_name = ip.pop()?;
    if len > 256 << 20 {
        return err("limitcheck");
    }
    let file =
        ip.e.iter()
            .rev()
            .find_map(|f| match f {
                crate::interp::Frame::File { id } => Some(*id),
                _ => None,
            })
            .unwrap_or(ip.main);
    ip.alloc(len)?;
    let mut data = Vec::with_capacity(len);
    while data.len() < len {
        match ip.srcs.read(file) {
            Some(c) => data.push(c),
            None => break,
        }
    }
    // StartData ends the FontSetInit dictionary its program began.
    let fsi = match category(ip, b"ProcSet").get_name(b"FontSetInit") {
        Some(Obj::Dict(d)) => Some(d),
        _ => None,
    };
    if let (Some(top), Some(fsi)) = (ip.d.last(), fsi)
        && top.same(&fsi)
        && ip.d.len() > 3
    {
        ip.d.pop();
    }
    // Parsing walks the data, and a font set's glyph tables are as large again.
    ip.charge_bytes(data.len())?;
    ip.alloc(data.len())?;
    let Some(set) = crate::cff::parse(data) else {
        ip.warnings.push("unreadable CFF font set".into());
        return Ok(());
    };
    let set = Rc::new(set);
    let si = ip.cff.len();
    ip.cff.push(set.clone());
    for (k, f) in set.fonts.iter().enumerate() {
        let d = PDict::new(16);
        let reference = Obj::Array(PArr::new(vec![Obj::Int(si as i64), Obj::Int(k as i64)]), false);
        d.put_name(b"DesignCraftCFF", reference);
        d.put_name(b"FontMatrix", nums(&f.matrix));
        d.put_name(b"FontBBox", nums(&f.bbox));
        d.put_name(b"PaintType", Obj::Int(0));
        if let Some((reg, ord, sup)) = &f.ros {
            d.put_name(b"CIDFontName", Obj::lit(&f.name));
            d.put_name(b"CIDFontType", Obj::Int(0));
            d.put_name(b"FontType", Obj::Int(9));
            let info = PDict::new(3);
            info.put_name(b"Registry", Obj::str(reg));
            info.put_name(b"Ordering", Obj::str(ord));
            info.put_name(b"Supplement", Obj::Int(*sup));
            d.put_name(b"CIDSystemInfo", Obj::Dict(info));
            add_fid(ip, &d);
            category(ip, b"CIDFont").put(Obj::lit(&f.name), Obj::Dict(d));
        } else {
            d.put_name(b"FontName", Obj::lit(&f.name));
            d.put_name(b"FontType", Obj::Int(2));
            let mut enc = vec![Obj::lit(b".notdef"); 256];
            for (code, gid) in &f.encoding {
                if let (Some(slot), Some(n)) = (enc.get_mut(usize::from(*code)), f.names.get(*gid as usize)) {
                    *slot = Obj::lit(n);
                }
            }
            d.put_name(b"Encoding", Obj::Array(PArr::new(enc), false));
            let cs = PDict::new(f.names.len());
            for (g, n) in f.names.iter().enumerate() {
                cs.put_name(n, Obj::Int(g as i64));
            }
            d.put_name(b"CharStrings", Obj::Dict(cs));
            d.put_name(b"Private", Obj::Dict(PDict::new(1)));
            add_fid(ip, &d);
            ip.fontdir.put(Obj::lit(&f.name), Obj::Dict(d.clone()));
            category(ip, b"Font").put(Obj::lit(&f.name), Obj::Dict(d));
        }
    }
    Ok(())
}

/// The CFF font set and font index behind a font dictionary.
pub fn cff_of(ip: &Interp, d: &PDict) -> Option<(Rc<crate::cff::FontSet>, usize)> {
    let Some(Obj::Array(a, _)) = d.get_name(b"DesignCraftCFF") else { return None };
    let si = a.get(0)?.num()? as usize;
    let k = a.get(1)?.num()? as usize;
    Some((ip.cff.get(si)?.clone(), k))
}

// ----- composite fonts -----

/// `name cmap [cidfonts] composefont`: a Type 0 font.
pub fn composefont(ip: &mut Interp) -> R {
    let fonts = ip.pop_arr()?;
    let cmap = ip.pop()?;
    let n = ip.pop()?;
    let font = compose(ip, &n, &cmap, fonts.items())?;
    ip.push(Obj::Dict(font))
}

fn compose(ip: &mut Interp, n: &Obj, cmap: &Obj, descendants: Vec<Obj>) -> R<PDict> {
    let i = find_cmap(ip, cmap)?;
    let mut desc = Vec::new();
    for f in descendants {
        desc.push(match f {
            Obj::Dict(d) => Obj::Dict(d),
            k => match category(ip, b"CIDFont").get(&k.key()) {
                Some(d @ Obj::Dict(_)) => d,
                _ => Obj::Dict(crate::font::find_font(ip, &k)),
            },
        });
    }
    let d = PDict::new(10);
    d.put_name(b"FontName", n.clone());
    d.put_name(b"FontType", Obj::Int(0));
    d.put_name(b"FMapType", Obj::Int(9));
    d.put_name(b"FontMatrix", matrix_obj(&M::ID));
    d.put_name(b"Encoding", Obj::Array(PArr::new(vec![Obj::Int(0)]), false));
    d.put_name(b"FDepVector", Obj::Array(PArr::new(desc), false));
    let cm = PDict::new(2);
    cm.put_name(b"DesignCraftCMap", Obj::Int(i as i64));
    d.put_name(b"CMap", Obj::Dict(cm));
    let vertical = ip.cmaps.get(i).is_some_and(|m| m.vertical);
    d.put_name(b"WMode", Obj::Int(i64::from(vertical)));
    add_fid(ip, &d);
    ip.fontdir.put(n.clone(), Obj::Dict(d.clone()));
    Ok(d)
}

/// `findfont` of `CIDFont-CMap` (as Adobe interpreters do): the two composed.
pub fn compose_by_name(ip: &mut Interp, n: &[u8]) -> Option<PDict> {
    let cats = category(ip, b"CIDFont");
    for (k, v) in cats.entries() {
        let kn = crate::ops::basic::text_of(&k);
        if n.len() > kn.len() + 1 && n.starts_with(&kn) && n.get(kn.len()) == Some(&b'-') {
            let cmap = n.get(kn.len() + 1..)?;
            return compose(ip, &Obj::lit(n), &Obj::lit(cmap), vec![v]).ok();
        }
    }
    None
}

/// Whether a font writes vertically (its WMode, or its CMap's).
pub fn vertical(ip: &Interp, font: &PDict) -> bool {
    match font.get_name(b"WMode").and_then(|o| o.num()) {
        Some(w) => w == 1.0,
        None => match font.get_name(b"CMap") {
            Some(Obj::Dict(d)) => ip.cmap_of(&d).and_then(|i| ip.cmaps.get(i)).is_some_and(|m| m.vertical),
            _ => false,
        },
    }
}

/// A CIDFont's vertical metrics for a CID in glyph space: the advance (w1) and the position
/// vector (v) from the horizontal origin to the vertical one. `Metrics2` gives them per CID;
/// otherwise the defaults: one em down, and v = (half the width, 0.88 em).
pub fn vertical_metrics(cidfont: &PDict, cid: u32, w0: f64, em: f64) -> ((f64, f64), (f64, f64)) {
    if let Some(Obj::Dict(m)) = cidfont.get_name(b"Metrics2")
        && let Some(Obj::Array(a, _)) = m.get(&Key::Int(i64::from(cid)))
    {
        let v: Vec<f64> = a.items().iter().filter_map(Obj::num).collect();
        if let [w1x, w1y, vx, vy] = v[..] {
            return ((w1x, w1y), (vx, vy));
        }
    }
    ((0.0, -em), (w0 / 2.0, 0.88 * em))
}

/// What one glyph of a shown string draws with.
pub enum Key2 {
    Code(u8),
    Cid(u32),
}

/// The glyphs of a string in a Type 0 font: (code, leaf font, glyph → text-space matrix, key).
pub fn decode(ip: &mut Interp, font: &PDict, s: &[u8]) -> R<Vec<(u32, PDict, M, Key2)>> {
    let fm = font_matrix(font);
    let desc: Vec<PDict> = match font.get_name(b"FDepVector") {
        Some(Obj::Array(a, _)) => a.items().into_iter().filter_map(|o| if let Obj::Dict(d) = o { Some(d) } else { None }).collect(),
        _ => Vec::new(),
    };
    let Some(first) = desc.first().cloned() else { return err("invalidfont") };
    let fmap = font.get_name(b"FMapType").and_then(|o| o.num()).unwrap_or(9.0) as i64;
    let mut out = Vec::new();
    if fmap == 2 {
        // 8/8 mapping: a font number, then a code in it.
        for p in s.chunks(2) {
            if let [f, c] = p {
                let leaf = desc.get(usize::from(*f)).cloned().unwrap_or_else(|| first.clone());
                let m = font_matrix(&leaf).then(&fm);
                out.push((u32::from(*c), leaf, m, Key2::Code(*c)));
            }
        }
        return Ok(out);
    }
    let cmap = match font.get_name(b"CMap") {
        Some(Obj::Dict(d)) => ip.cmap_of(&d),
        _ => None,
    };
    let m = font_matrix(&first).then(&fm);
    let mut i = 0;
    // Each code is looked up through the CMap's ranges (and its parents').
    let per_code = cmap.map_or(1, |c| ip.cmap_cost(c)) / 16 + 1;
    while i < s.len() {
        ip.charge(per_code)?;
        let rest = s.get(i..).unwrap_or(&[]);
        let n = cmap.and_then(|c| ip.code_len(c, rest, 0)).unwrap_or(2).min(rest.len()).max(1);
        let code = rest.get(..n).unwrap_or(&[]);
        let cid = cmap.and_then(|c| ip.cid(c, code, 0)).unwrap_or(0);
        out.push((u32::try_from(code_value(code)).unwrap_or(0), first.clone(), m, Key2::Cid(cid)));
        i += n;
    }
    Ok(out)
}

fn font_matrix(d: &PDict) -> M {
    match d.get_name(b"FontMatrix") {
        Some(Obj::Array(a, _)) => crate::interp::matrix_of(&a).unwrap_or(M([0.001, 0.0, 0.0, 0.001, 0.0, 0.0])),
        _ => M([0.001, 0.0, 0.0, 0.001, 0.0, 0.0]),
    }
}

/// A CID's glyph in a CIDFont: CFF (CIDFontType 0) or TrueType (CIDFontType 2, through CIDMap).
pub fn cid_glyph(ip: &mut Interp, cidfont: &PDict, cid: u32) -> Option<Glyph> {
    if let Some((set, k)) = cff_of(ip, cidfont) {
        let gid = set.gid_of_cid(k, cid).unwrap_or(0);
        return set.glyph(k, gid);
    }
    if cidfont.get_name(b"CIDFontType").and_then(|o| o.num()) == Some(2.0) {
        let gid = match cidfont.get_name(b"CIDMap") {
            Some(Obj::Int(off)) => u32::try_from(i64::from(cid) + off).ok()?,
            Some(Obj::Dict(m)) => m.get(&Key::Int(i64::from(cid))).and_then(|o| o.num()).map_or(0, |v| v as u32),
            Some(Obj::Str(s, _)) => gid_from(&[&s], cidfont, cid),
            Some(Obj::Array(a, _)) => {
                let strs: Vec<PStr> = a.items().into_iter().filter_map(|o| if let Obj::Str(s, _) = o { Some(s) } else { None }).collect();
                gid_from(&strs.iter().collect::<Vec<_>>(), cidfont, cid)
            }
            _ => cid,
        };
        let data = crate::type42::sfnts(cidfont)?;
        return crate::type42::glyph(&data, gid);
    }
    None
}

/// A glyph index read from CIDMap strings (GDBytes bytes per CID, across the strings).
fn gid_from(strs: &[&PStr], cidfont: &PDict, cid: u32) -> u32 {
    let gd = cidfont.get_name(b"GDBytes").and_then(|o| o.num()).unwrap_or(2.0).clamp(1.0, 4.0) as usize;
    let mut at = (cid as usize).saturating_mul(gd);
    for s in strs {
        if at < s.len {
            return (0..gd).fold(0u32, |a, k| a << 8 | u32::from(s.get(at + k).unwrap_or(0)));
        }
        at -= s.len;
    }
    0
}

/// The ProcSets EPS files ask for, and the identity CMaps.
pub fn install(ip: &mut Interp) {
    let procsets = category(ip, b"ProcSet");
    let fsi = PDict::new(2);
    if let Some(op) = crate::ops::op_id(b"StartData") {
        fsi.put_name(b"StartData", Obj::Op(op));
    }
    procsets.put_name(b"FontSetInit", Obj::Dict(fsi));
    let cid = PDict::new(24);
    for n in [
        "begincmap",
        "endcmap",
        "usecmap",
        "begincodespacerange",
        "endcodespacerange",
        "begincidrange",
        "endcidrange",
        "begincidchar",
        "endcidchar",
        "beginbfchar",
        "endbfchar",
        "beginbfrange",
        "endbfrange",
        "beginnotdefrange",
        "endnotdefrange",
        "beginnotdefchar",
        "endnotdefchar",
    ] {
        if let Some(op) = crate::ops::op_id(n.as_bytes()) {
            cid.put_name(n.as_bytes(), Obj::Op(op));
        }
    }
    procsets.put_name(b"CIDInit", Obj::Dict(cid));
    let cmaps = category(ip, b"CMap");
    for n in [&b"Identity-H"[..], b"Identity-V"] {
        let d = identity(ip, n);
        cmaps.put_name(n, Obj::Dict(d));
    }
}
