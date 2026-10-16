//! CFF font sets and Type 2 charstrings (Adobe Technical Notes 5176 and 5177): name-keyed and
//! CID-keyed fonts, as `StartData` defines them.

use crate::gstate::Seg;
use crate::type1::Glyph;

/// The most glyphs, subroutines or INDEX entries read.
const MAX_ENTRIES: usize = 200_000;
const MAX_OPS: usize = 200_000;
const MAX_DEPTH: usize = 10;

#[derive(Debug, Clone, Default)]
struct Private {
    subrs: Vec<(usize, usize)>,
    default_width: f64,
    nominal_width: f64,
}

#[derive(Debug, Clone)]
pub struct Font {
    pub name: Vec<u8>,
    pub matrix: [f64; 6],
    pub bbox: [f64; 4],
    /// Glyph names (name-keyed) by glyph index.
    pub names: Vec<Vec<u8>>,
    /// Code → glyph index (name-keyed fonts).
    pub encoding: Vec<(u8, u32)>,
    /// CID by glyph index (CID-keyed fonts), and the registry/ordering/supplement.
    pub cids: Option<Vec<u32>>,
    pub ros: Option<(Vec<u8>, Vec<u8>, i64)>,
    /// CID → glyph index, made on first lookup.
    by_cid: std::cell::OnceCell<std::collections::HashMap<u32, u32>>,
    charstrings: Vec<(usize, usize)>,
    privates: Vec<Private>,
    fd_select: Vec<u8>,
}

#[derive(Debug)]
pub struct FontSet {
    pub data: Vec<u8>,
    gsubrs: Vec<(usize, usize)>,
    pub fonts: Vec<Font>,
}

fn u16_at(d: &[u8], i: usize) -> Option<usize> {
    Some(usize::from(u16::from_be_bytes([*d.get(i)?, *d.get(i + 1)?])))
}

fn offset(d: &[u8], i: usize, size: usize) -> Option<usize> {
    let mut v = 0usize;
    for k in 0..size {
        v = v.checked_mul(256)?.checked_add(usize::from(*d.get(i + k)?))?;
    }
    Some(v)
}

/// An INDEX at `at`: its entries as (start, end) in `d`, and where it ends.
fn index(d: &[u8], at: usize) -> Option<(Vec<(usize, usize)>, usize)> {
    let count = u16_at(d, at)?;
    if count == 0 {
        return Some((Vec::new(), at + 2));
    }
    if count > MAX_ENTRIES {
        return None;
    }
    let size = usize::from(*d.get(at + 2)?);
    if !(1..=4).contains(&size) {
        return None;
    }
    let offs_at = at + 3;
    let data_at = offs_at + (count + 1) * size - 1;
    let mut out = Vec::with_capacity(count);
    let mut prev = offset(d, offs_at, size)?;
    for i in 1..=count {
        let next = offset(d, offs_at + i * size, size)?;
        let (s, e) = (data_at.checked_add(prev)?, data_at.checked_add(next)?);
        if e < s || e > d.len() {
            return None;
        }
        out.push((s, e));
        prev = next;
    }
    let end = data_at.checked_add(prev)?;
    Some((out, end))
}

#[derive(Debug, Default)]
struct Dict {
    entries: Vec<(u16, Vec<f64>)>,
}

impl Dict {
    fn get(&self, op: u16) -> Option<&[f64]> {
        self.entries.iter().find(|(o, _)| *o == op).map(|(_, v)| v.as_slice())
    }

    fn num(&self, op: u16) -> Option<f64> {
        self.get(op)?.first().copied()
    }
}

fn dict(d: &[u8]) -> Dict {
    let mut out = Dict::default();
    let mut ops: Vec<f64> = Vec::new();
    let mut i = 0;
    while let Some(&b) = d.get(i) {
        i += 1;
        match b {
            0..=21 => {
                let op = if b == 12 {
                    let e = d.get(i).copied().unwrap_or(0);
                    i += 1;
                    1200 + u16::from(e)
                } else {
                    u16::from(b)
                };
                out.entries.push((op, std::mem::take(&mut ops)));
            }
            28 => {
                let v = i16::from_be_bytes([d.get(i).copied().unwrap_or(0), d.get(i + 1).copied().unwrap_or(0)]);
                i += 2;
                ops.push(f64::from(v));
            }
            29 => {
                let v = i32::from_be_bytes([0, 1, 2, 3].map(|k| d.get(i + k).copied().unwrap_or(0)));
                i += 4;
                ops.push(f64::from(v));
            }
            30 => {
                // A real: nibbles.
                let mut s = String::new();
                'real: while let Some(&n) = d.get(i) {
                    i += 1;
                    for nib in [n >> 4, n & 15] {
                        match nib {
                            0..=9 => s.push(char::from(b'0' + nib)),
                            0xa => s.push('.'),
                            0xb => s.push('E'),
                            0xc => s.push_str("E-"),
                            0xe => s.push('-'),
                            0xf => break 'real,
                            _ => {}
                        }
                    }
                }
                ops.push(s.parse().unwrap_or(0.0));
            }
            32..=246 => ops.push(f64::from(b) - 139.0),
            247..=250 => {
                let w = d.get(i).copied().unwrap_or(0);
                i += 1;
                ops.push((f64::from(b) - 247.0) * 256.0 + f64::from(w) + 108.0);
            }
            251..=254 => {
                let w = d.get(i).copied().unwrap_or(0);
                i += 1;
                ops.push(-(f64::from(b) - 251.0) * 256.0 - f64::from(w) - 108.0);
            }
            _ => {}
        }
        if ops.len() > 64 {
            break;
        }
    }
    out
}

fn private(d: &[u8], top: &Dict) -> Private {
    let Some(v) = top.get(18) else { return Private::default() };
    let (size, off) = (v.first().copied().unwrap_or(0.0).max(0.0) as usize, v.get(1).copied().unwrap_or(0.0).max(0.0) as usize);
    let Some(bytes) = off.checked_add(size).and_then(|e| d.get(off..e)) else { return Private::default() };
    let p = dict(bytes);
    let subrs = p.num(19).and_then(|rel| index(d, off.checked_add(rel.max(0.0) as usize)?)).map(|(v, _)| v).unwrap_or_default();
    Private { subrs, default_width: p.num(20).unwrap_or(0.0), nominal_width: p.num(21).unwrap_or(0.0) }
}

/// The glyph index → SID (or CID) charset.
fn charset(d: &[u8], at: usize, glyphs: usize) -> Vec<u32> {
    let mut out = vec![0u32];
    // Predefined charsets (ISOAdobe and the expert ones): glyph index = SID.
    if at <= 2 {
        return (0..glyphs as u32).collect();
    }
    let Some(&format) = d.get(at) else { return out };
    let mut i = at + 1;
    while out.len() < glyphs {
        match format {
            0 => match u16_at(d, i) {
                Some(s) => {
                    out.push(s as u32);
                    i += 2;
                }
                None => break,
            },
            1 | 2 => {
                let (Some(first), Some(n)) = (u16_at(d, i), if format == 1 { d.get(i + 2).map(|v| usize::from(*v)) } else { u16_at(d, i + 2) })
                else {
                    break;
                };
                i += if format == 1 { 3 } else { 4 };
                for k in 0..=n {
                    if out.len() >= glyphs {
                        break;
                    }
                    out.push((first + k) as u32);
                }
            }
            _ => break,
        }
    }
    out
}

fn fd_select(d: &[u8], at: usize, glyphs: usize) -> Vec<u8> {
    let mut out = vec![0u8; glyphs];
    match d.get(at) {
        Some(0) => {
            for (g, slot) in out.iter_mut().enumerate() {
                *slot = d.get(at + 1 + g).copied().unwrap_or(0);
            }
        }
        Some(3) => {
            let n = u16_at(d, at + 1).unwrap_or(0);
            for r in 0..n {
                let base = at + 3 + r * 3;
                let (Some(first), Some(fd), Some(next)) = (u16_at(d, base), d.get(base + 2), u16_at(d, base + 3)) else { break };
                for g in first..next.min(glyphs) {
                    if let Some(slot) = out.get_mut(g) {
                        *slot = *fd;
                    }
                }
            }
        }
        _ => {}
    }
    out
}

fn sid_name(sid: u32, strings: &[(usize, usize)], d: &[u8]) -> Vec<u8> {
    let sid = sid as usize;
    match crate::cff_strings::standard(sid) {
        Some(n) => n.into_bytes(),
        None => {
            strings.get(sid - crate::cff_strings::COUNT).and_then(|(s, e)| d.get(*s..*e)).map(<[u8]>::to_vec).unwrap_or_else(|| b".notdef".to_vec())
        }
    }
}

pub fn parse(data: Vec<u8>) -> Option<FontSet> {
    let d = &data;
    let hdr = usize::from(*d.get(2)?);
    let (names, after_names) = index(d, hdr)?;
    let (tops, after_tops) = index(d, after_names)?;
    let (strings, after_strings) = index(d, after_tops)?;
    let (gsubrs, _) = index(d, after_strings)?;
    let mut fonts = Vec::new();
    for (k, &(ts, te)) in tops.iter().enumerate() {
        let top = dict(d.get(ts..te)?);
        let name = names.get(k).and_then(|(s, e)| d.get(*s..*e)).map(<[u8]>::to_vec).unwrap_or_default();
        let (charstrings, _) = index(d, top.num(17)?.max(0.0) as usize)?;
        let glyphs = charstrings.len();
        let matrix =
            top.get(1207).filter(|m| m.len() == 6).map(|m| [m[0], m[1], m[2], m[3], m[4], m[5]]).unwrap_or([0.001, 0.0, 0.0, 0.001, 0.0, 0.0]);
        let bbox = top.get(5).filter(|b| b.len() == 4).map(|b| [b[0], b[1], b[2], b[3]]).unwrap_or([0.0, 0.0, 1000.0, 1000.0]);
        let cs = charset(d, top.num(15).unwrap_or(0.0).max(0.0) as usize, glyphs);
        let ros = top.get(1230).map(|v| {
            let s = |i: usize| sid_name(v.get(i).copied().unwrap_or(0.0) as u32, &strings, d);
            (s(0), s(1), v.get(2).copied().unwrap_or(0.0) as i64)
        });
        let (privates, fds, names_out, cids) = if ros.is_some() {
            let fdarray = top.num(1236).and_then(|o| index(d, o.max(0.0) as usize)).map(|(v, _)| v).unwrap_or_default();
            let privates = fdarray.iter().map(|(s, e)| d.get(*s..*e).map(|b| private(d, &dict(b))).unwrap_or_default()).collect();
            let fds = top.num(1237).map(|o| fd_select(d, o.max(0.0) as usize, glyphs)).unwrap_or_else(|| vec![0; glyphs]);
            (privates, fds, Vec::new(), Some(cs))
        } else {
            let names_out: Vec<Vec<u8>> = cs.iter().map(|sid| sid_name(*sid, &strings, d)).collect();
            (vec![private(d, &top)], Vec::new(), names_out, None)
        };
        // Encoding (name-keyed): predefined Standard, or format 0/1 code lists.
        let mut encoding = Vec::new();
        if cids.is_none() {
            match top.num(16).unwrap_or(0.0) as usize {
                0 => {
                    let std = crate::encodings::standard();
                    for (code, n) in std.iter().enumerate() {
                        if let Some(g) = names_out.iter().position(|x| x == n.as_bytes()) {
                            encoding.push((code as u8, g as u32));
                        }
                    }
                }
                1 => {}
                at => {
                    let fmt = d.get(at).copied().unwrap_or(0) & 0x7f;
                    let n = usize::from(d.get(at + 1).copied().unwrap_or(0));
                    if fmt == 0 {
                        for g in 0..n {
                            if let Some(c) = d.get(at + 2 + g) {
                                encoding.push((*c, g as u32 + 1));
                            }
                        }
                    } else {
                        let mut g = 1u32;
                        for r in 0..n {
                            let (Some(first), Some(left)) = (d.get(at + 2 + r * 2), d.get(at + 3 + r * 2)) else { break };
                            for k in 0..=u32::from(*left) {
                                encoding.push((first.wrapping_add(k as u8), g));
                                g += 1;
                            }
                        }
                    }
                }
            }
        }
        fonts.push(Font {
            name,
            matrix,
            bbox,
            names: names_out,
            encoding,
            cids,
            ros,
            by_cid: std::cell::OnceCell::new(),
            charstrings,
            privates,
            fd_select: fds,
        });
    }
    Some(FontSet { data, gsubrs, fonts })
}

fn bias(n: usize) -> i64 {
    if n < 1240 {
        107
    } else if n < 33900 {
        1131
    } else {
        32768
    }
}

struct Run {
    path: Vec<Seg>,
    x: f64,
    y: f64,
    open: bool,
    stack: Vec<f64>,
    hints: usize,
    width: Option<f64>,
    ops: usize,
    done: bool,
}

impl Run {
    fn move_to(&mut self, dx: f64, dy: f64) {
        if self.open {
            self.path.push(Seg::Close);
        }
        self.x += dx;
        self.y += dy;
        self.path.push(Seg::Move(self.x, self.y));
        self.open = true;
    }

    fn line(&mut self, dx: f64, dy: f64) {
        self.x += dx;
        self.y += dy;
        self.path.push(Seg::Line(self.x, self.y));
    }

    fn curve(&mut self, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) {
        let (x1, y1) = (self.x + a, self.y + b);
        let (x2, y2) = (x1 + c, y1 + d);
        let (x3, y3) = (x2 + e, y2 + f);
        self.path.push(Seg::Curve(x1, y1, x2, y2, x3, y3));
        self.x = x3;
        self.y = y3;
    }

    /// The first stack-clearing operator may carry the width before its arguments: there is
    /// one when the count's parity differs from the operator's own (`expected_odd`).
    fn take_width(&mut self, expected_odd: bool) {
        if self.width.is_none() {
            let present = !self.stack.is_empty() && (self.stack.len() % 2 == 1) != expected_odd;
            self.width = Some(if present { self.stack.remove(0) } else { f64::NAN });
        }
    }
}

impl FontSet {
    fn bytes(&self, r: (usize, usize)) -> &[u8] {
        self.data.get(r.0..r.1).unwrap_or(&[])
    }

    fn exec(&self, code: &[u8], local: &[(usize, usize)], run: &mut Run, depth: usize) -> Option<()> {
        if depth > MAX_DEPTH {
            return None;
        }
        let mut i = 0;
        while i < code.len() && !run.done {
            run.ops += 1;
            if run.ops > MAX_OPS || run.stack.len() > 96 {
                return None;
            }
            let b = *code.get(i)?;
            i += 1;
            match b {
                32..=246 => run.stack.push(f64::from(b) - 139.0),
                247..=250 => {
                    let w = *code.get(i)?;
                    i += 1;
                    run.stack.push((f64::from(b) - 247.0) * 256.0 + f64::from(w) + 108.0);
                }
                251..=254 => {
                    let w = *code.get(i)?;
                    i += 1;
                    run.stack.push(-(f64::from(b) - 251.0) * 256.0 - f64::from(w) - 108.0);
                }
                28 => {
                    let v = i16::from_be_bytes([*code.get(i)?, *code.get(i + 1)?]);
                    i += 2;
                    run.stack.push(f64::from(v));
                }
                255 => {
                    let v = i32::from_be_bytes([*code.get(i)?, *code.get(i + 1)?, *code.get(i + 2)?, *code.get(i + 3)?]);
                    i += 4;
                    run.stack.push(f64::from(v) / 65536.0);
                }
                // hstem, vstem, hstemhm, vstemhm: count the hints (an odd count carries the width).
                1 | 3 | 18 | 23 => {
                    run.take_width(false);
                    run.hints += run.stack.len() / 2;
                    run.stack.clear();
                }
                19 | 20 => {
                    // hintmask, cntrmask: implicit vstems, then the mask bytes.
                    run.take_width(false);
                    run.hints += run.stack.len() / 2;
                    run.stack.clear();
                    i += run.hints.div_ceil(8);
                }
                21 => {
                    run.take_width(false);
                    let s = std::mem::take(&mut run.stack);
                    run.move_to(s.first().copied().unwrap_or(0.0), s.get(1).copied().unwrap_or(0.0));
                }
                22 | 4 => {
                    run.take_width(true);
                    let s = std::mem::take(&mut run.stack);
                    let v = s.first().copied().unwrap_or(0.0);
                    if b == 22 { run.move_to(v, 0.0) } else { run.move_to(0.0, v) }
                }
                5 => {
                    let s = std::mem::take(&mut run.stack);
                    for &[x, y] in s.as_chunks::<2>().0 {
                        run.line(x, y);
                    }
                }
                6 | 7 => {
                    let s = std::mem::take(&mut run.stack);
                    let mut horizontal = b == 6;
                    for v in s {
                        if horizontal {
                            run.line(v, 0.0)
                        } else {
                            run.line(0.0, v)
                        }
                        horizontal = !horizontal;
                    }
                }
                8 => {
                    let s = std::mem::take(&mut run.stack);
                    for &[a, b2, c, d, e, f] in s.as_chunks::<6>().0 {
                        run.curve(a, b2, c, d, e, f);
                    }
                }
                24 => {
                    // rcurveline
                    let s = std::mem::take(&mut run.stack);
                    let n = s.len().saturating_sub(2) / 6;
                    for &[a, b2, c, d, e, f] in s.as_chunks::<6>().0.iter().take(n) {
                        run.curve(a, b2, c, d, e, f);
                    }
                    if let (Some(a), Some(b2)) = (s.get(n * 6), s.get(n * 6 + 1)) {
                        run.line(*a, *b2);
                    }
                }
                25 => {
                    // rlinecurve
                    let s = std::mem::take(&mut run.stack);
                    let lines = s.len().saturating_sub(6) / 2;
                    for &[x, y] in s.as_chunks::<2>().0.iter().take(lines) {
                        run.line(x, y);
                    }
                    if let Some(&[a, b2, c, d, e, f]) = s.get(lines * 2..lines * 2 + 6).and_then(|w| <&[f64; 6]>::try_from(w).ok()) {
                        run.curve(a, b2, c, d, e, f);
                    }
                }
                26 | 27 => {
                    // vvcurveto, hhcurveto
                    let mut s = std::mem::take(&mut run.stack);
                    let mut first = if s.len() % 4 == 1 { Some(s.remove(0)) } else { None };
                    for &[p, q, r, t] in s.as_chunks::<4>().0 {
                        let d1 = first.take().unwrap_or(0.0);
                        if b == 26 { run.curve(d1, p, q, r, 0.0, t) } else { run.curve(p, d1, q, r, t, 0.0) }
                    }
                }
                30 | 31 => {
                    // vhcurveto, hvcurveto: alternating curves; the last may end off-axis.
                    let s = std::mem::take(&mut run.stack);
                    let mut horizontal = b == 31;
                    let mut k = 0;
                    while let Some(&[a, bb, c, d]) = s.get(k..k + 4).and_then(|w| <&[f64; 4]>::try_from(w).ok()) {
                        let extra = if s.len() - k == 5 { s.get(k + 4).copied().unwrap_or(0.0) } else { 0.0 };
                        if horizontal {
                            run.curve(a, 0.0, bb, c, extra, d)
                        } else {
                            run.curve(0.0, a, bb, c, d, extra)
                        }
                        horizontal = !horizontal;
                        k += 4;
                    }
                }
                10 | 29 => {
                    let n = run.stack.pop()? as i64;
                    let (list, bias) = if b == 10 { (local, bias(local.len())) } else { (&self.gsubrs[..], bias(self.gsubrs.len())) };
                    let sub = *list.get(usize::try_from(n + bias).ok()?)?;
                    let sub = self.bytes(sub).to_vec();
                    self.exec(&sub, local, run, depth + 1)?;
                }
                11 => return Some(()),
                14 => {
                    run.take_width(false);
                    if run.open {
                        run.path.push(Seg::Close);
                        run.open = false;
                    }
                    run.done = true;
                }
                12 => {
                    let e = *code.get(i)?;
                    i += 1;
                    let s = std::mem::take(&mut run.stack);
                    let a = |k: usize| s.get(k).copied().unwrap_or(0.0);
                    match e {
                        35 => {
                            run.curve(a(0), a(1), a(2), a(3), a(4), a(5));
                            run.curve(a(6), a(7), a(8), a(9), a(10), a(11));
                        }
                        34 => {
                            // hflex: dx1 dx2 dy2 dx3 dx4 dx5 dx6, back to the start height.
                            run.curve(a(0), 0.0, a(1), a(2), a(3), 0.0);
                            run.curve(a(4), 0.0, a(5), -a(2), a(6), 0.0);
                        }
                        36 => {
                            // hflex1: dx1 dy1 dx2 dy2 dx3 dx4 dx5 dy5 dx6.
                            run.curve(a(0), a(1), a(2), a(3), a(4), 0.0);
                            run.curve(a(5), 0.0, a(6), a(7), a(8), -(a(1) + a(3) + a(7)));
                        }
                        37 => {
                            // flex1: the last coordinate runs along the longer direction.
                            let dx = a(0) + a(2) + a(4) + a(6) + a(8);
                            let dy = a(1) + a(3) + a(5) + a(7) + a(9);
                            let (ex, ey) = if dx.abs() > dy.abs() { (a(10), -dy) } else { (-dx, a(10)) };
                            run.curve(a(0), a(1), a(2), a(3), a(4), a(5));
                            run.curve(a(6), a(7), a(8), a(9), ex, ey);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        Some(())
    }

    /// Glyph `gid` of font `k` in glyph space (multiplied out by its FontMatrix later).
    pub fn glyph(&self, k: usize, gid: u32) -> Option<Glyph> {
        let font = self.fonts.get(k)?;
        let cs = *font.charstrings.get(gid as usize)?;
        let fd = usize::from(font.fd_select.get(gid as usize).copied().unwrap_or(0));
        let private = font.privates.get(fd).or_else(|| font.privates.first()).cloned().unwrap_or_default();
        let mut run = Run { path: Vec::new(), x: 0.0, y: 0.0, open: false, stack: Vec::new(), hints: 0, width: None, ops: 0, done: false };
        let code = self.bytes(cs).to_vec();
        self.exec(&code, &private.subrs, &mut run, 0)?;
        if run.open {
            run.path.push(Seg::Close);
        }
        let w = match run.width {
            Some(w) if w.is_finite() => private.nominal_width + w,
            _ => private.default_width,
        };
        Some(Glyph { cost: run.ops + run.path.len(), path: run.path, width: (w, 0.0) })
    }

    /// The glyph index of a CID in font `k`.
    pub fn gid_of_cid(&self, k: usize, cid: u32) -> Option<u32> {
        let font = self.fonts.get(k)?;
        let cids = font.cids.as_ref()?;
        let map = font.by_cid.get_or_init(|| cids.iter().enumerate().filter_map(|(g, c)| Some((*c, u32::try_from(g).ok()?))).collect());
        map.get(&cid).copied()
    }
}
