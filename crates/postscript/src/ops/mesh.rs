//! Mesh shadings (types 4–7): free-form and lattice triangles, Coons and tensor patches, read
//! from an array, a string or a file, their colours made device colours, and written to the PDF
//! as meshes again (each triangle and patch whole, so shared edges need no state there).

use crate::gstate::{DevColor, Space};
use crate::interp::{Interp, R, err};
use crate::obj::{Obj, PDict};
use crate::ops::gfx;
use crate::ops::shading::{Func, eval, pack};
use crate::pdf::{ShadeKind, Shading};

/// The most vertices or patches read from one shading.
const MAX_ELEMENTS: usize = 2_000_000;

/// The numbers of a mesh: from an array as they are, or unpacked from bits with Decode.
struct Reader {
    nums: Vec<f64>,
    bits: Option<(Vec<u8>, usize)>,
    pos: usize,
    bpc: usize,
    bpcomp: usize,
    bpflag: usize,
    decode: Vec<f64>,
}

impl Reader {
    fn raw(&mut self, n: usize) -> Option<u64> {
        let (data, bit) = self.bits.as_mut()?;
        let mut v = 0u64;
        for _ in 0..n {
            let byte = *data.get(*bit / 8)?;
            v = v << 1 | u64::from(byte >> (7 - *bit % 8) & 1);
            *bit += 1;
        }
        Some(v)
    }

    fn align(&mut self) {
        if let Some((_, bit)) = self.bits.as_mut() {
            *bit = bit.div_ceil(8) * 8;
        }
    }

    fn array_next(&mut self) -> Option<f64> {
        let v = self.nums.get(self.pos).copied();
        self.pos += 1;
        v
    }

    fn decoded(&mut self, bits: usize, range: usize) -> Option<f64> {
        if self.bits.is_none() {
            return self.array_next();
        }
        let v = self.raw(bits)?;
        let max = ((1u128 << bits) - 1) as f64;
        let (d0, d1) = (self.decode.get(2 * range).copied().unwrap_or(0.0), self.decode.get(2 * range + 1).copied().unwrap_or(1.0));
        Some(d0 + v as f64 / max * (d1 - d0))
    }

    fn flag(&mut self) -> Option<u8> {
        if self.bits.is_none() {
            return self.array_next().map(|v| v as u8);
        }
        self.raw(self.bpflag).map(|v| v as u8)
    }

    fn point(&mut self) -> Option<(f64, f64)> {
        let b = self.bpc;
        Some((self.decoded(b, 0)?, self.decoded(b, 1)?))
    }

    fn color(&mut self, n: usize) -> Option<Vec<f64>> {
        let b = self.bpcomp;
        (0..n).map(|i| self.decoded(b, 2 + i)).collect()
    }
}

pub fn build(ip: &mut Interp, d: &PDict, kind: i64, space: &Space, func: Option<&Func>) -> R<Option<Shading>> {
    let num = |k: &[u8]| d.get_name(k).and_then(|o| o.num()).unwrap_or(0.0);
    let ncomp = if func.is_some() { 1 } else { space.comps() };
    let decode: Vec<f64> = match d.get_name(b"Decode") {
        Some(Obj::Array(a, _)) => a.items().iter().filter_map(Obj::num).collect(),
        _ => Vec::new(),
    };
    let mut r = Reader {
        nums: Vec::new(),
        bits: None,
        pos: 0,
        bpc: num(b"BitsPerCoordinate").clamp(1.0, 32.0) as usize,
        bpcomp: num(b"BitsPerComponent").clamp(1.0, 16.0) as usize,
        bpflag: num(b"BitsPerFlag").clamp(2.0, 8.0) as usize,
        decode,
    };
    match d.get_name(b"DataSource") {
        Some(Obj::Array(a, _)) => r.nums = a.items().iter().filter_map(Obj::num).collect(),
        Some(Obj::Str(s, _)) => r.bits = Some((s.bytes(), 0)),
        Some(Obj::File(id)) => {
            let mut v = Vec::new();
            while let Some(c) = ip.srcs.read(id) {
                v.push(c);
                if v.len() > 64 << 20 {
                    break;
                }
            }
            r.bits = Some((v, 0));
        }
        _ => return err("typecheck"),
    }
    let to_dev = |ip: &mut Interp, c: &[f64]| -> R<DevColor> {
        let comps = match func {
            Some(f) => eval(ip, f, c, 0)?,
            None => c.to_vec(),
        };
        gfx::to_device(ip, space, &comps, 0)
    };
    let mut points: Vec<(f64, f64)> = Vec::new();
    let mut colors: Vec<DevColor> = Vec::new();
    let shade = match kind {
        4 | 5 => {
            // Vertices, then triangles of them.
            let mut verts: Vec<((f64, f64), DevColor)> = Vec::new();
            let mut tris: Vec<[usize; 3]> = Vec::new();
            let per_row = num(b"VerticesPerRow").max(2.0) as usize;
            let mut pending: Vec<usize> = Vec::new();
            while verts.len() < MAX_ELEMENTS {
                ip.charge(1)?;
                let flag = if kind == 4 {
                    match r.flag() {
                        Some(f) => f,
                        None => break,
                    }
                } else {
                    0
                };
                let (Some(p), Some(c)) = (r.point(), r.color(ncomp)) else { break };
                if kind == 4 && r.bits.is_some() {
                    r.align();
                }
                let c = to_dev(ip, &c)?;
                verts.push((p, c));
                let n = verts.len() - 1;
                if kind != 4 {
                    continue;
                }
                // Flag 0 starts a triangle of three new vertices; 1 and 2 continue from the
                // last triangle's (b, c) or (a, c) edge.
                match (flag, tris.last().copied()) {
                    (1, Some([_, b, c])) => tris.push([b, c, n]),
                    (2, Some([a, _, c])) => tris.push([a, c, n]),
                    _ => {
                        pending.push(n);
                        if pending.len() == 3 {
                            tris.push([pending[0], pending[1], pending[2]]);
                            pending.clear();
                        }
                    }
                }
            }
            if kind == 5 {
                let rows = verts.len() / per_row;
                for row in 0..rows.saturating_sub(1) {
                    for i in 0..per_row - 1 {
                        let a = row * per_row + i;
                        let (b, c, e) = (a + 1, a + per_row, a + per_row + 1);
                        tris.push([a, b, c]);
                        tris.push([b, e, c]);
                    }
                }
            }
            for t in tris {
                for i in t {
                    let Some((p, c)) = verts.get(i) else { return err("rangecheck") };
                    points.push(*p);
                    colors.push(*c);
                }
            }
            ShadeKind::Triangles
        }
        _ => {
            // Coons (12 control points) and tensor (16) patches; flags 1–3 share an edge.
            let npts = if kind == 6 { 12 } else { 16 };
            let mut prev: Option<(Vec<(f64, f64)>, Vec<DevColor>)> = None;
            let mut count = 0;
            loop {
                if count >= MAX_ELEMENTS {
                    break;
                }
                ip.charge(4)?;
                let Some(flag) = r.flag() else { break };
                let (shared_pts, shared_cols) = match (flag, &prev) {
                    (1..=3, Some((pp, pc))) => {
                        // The edge of the previous patch this one starts from (per the patch order).
                        let (pi, ci): (&[usize], [usize; 2]) = match flag {
                            1 => (&[3, 4, 5, 6], [1, 2]),
                            2 => (&[6, 7, 8, 9], [2, 3]),
                            _ => (&[9, 10, 11, 0], [3, 0]),
                        };
                        (
                            pi.iter().filter_map(|i| pp.get(*i).copied()).collect::<Vec<_>>(),
                            ci.iter().filter_map(|i| pc.get(*i).copied()).collect::<Vec<_>>(),
                        )
                    }
                    _ => (Vec::new(), Vec::new()),
                };
                let new_pts = if shared_pts.len() == 4 { npts - 4 } else { npts };
                let new_cols = if shared_cols.len() == 2 { 2 } else { 4 };
                let mut pts = shared_pts.clone();
                if pts.len() != 4 {
                    pts.clear();
                }
                let mut ok = true;
                for _ in 0..new_pts {
                    match r.point() {
                        Some(p) => pts.push(p),
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                let mut cols = if shared_cols.len() == 2 { shared_cols } else { Vec::new() };
                for _ in 0..new_cols {
                    match r.color(ncomp) {
                        Some(c) => cols.push(to_dev(ip, &c)?),
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                if r.bits.is_some() {
                    r.align();
                }
                if !ok || pts.len() != npts || cols.len() != 4 {
                    break;
                }
                points.extend(&pts);
                colors.extend(&cols);
                prev = Some((pts, cols));
                count += 1;
            }
            if kind == 6 { ShadeKind::Coons } else { ShadeKind::Tensor }
        }
    };
    if points.is_empty() {
        return Ok(None);
    }
    let bbox = match d.get_name(b"BBox") {
        Some(Obj::Array(a, _)) => {
            let v: Vec<f64> = a.items().iter().filter_map(Obj::num).collect();
            <[f64; 4]>::try_from(v).ok()
        }
        _ => None,
    };
    let (space, data) = pack(&colors);
    let n = colors.len();
    Ok(Some(Shading { kind: shade, extend: [false, false], space, data, samples: n, bbox, points }))
}
