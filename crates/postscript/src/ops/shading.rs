//! Smooth shading: `shfill` and shading patterns. Functions (types 0, 2 and 3, or procedures)
//! are evaluated here in the shading's colour space and sampled into device colours, which the
//! PDF carries as a sampled function.

use crate::gstate::{DevColor, M, Seg};
use crate::interp::{Interp, PsErr, R, err, matrix_of};
use crate::obj::{Obj, PDict};
use crate::ops::gfx;
use crate::pdf::{OutSpace, ShadeKind, Shading};

/// Samples along an axial or radial shading, and per side of a function-based one.
const SAMPLES: usize = 256;
const GRID: usize = 48;
/// The most inputs a sampled function may take.
const MAX_INPUTS: usize = 4;
/// The most samples a sampled function may hold.
const MAX_SAMPLES: usize = 1 << 22;

pub enum Func {
    Exp {
        c0: Vec<f64>,
        c1: Vec<f64>,
        n: f64,
        domain: [f64; 2],
    },
    Stitch {
        fns: Vec<Func>,
        bounds: Vec<f64>,
        encode: Vec<f64>,
        domain: [f64; 2],
    },
    Sampled {
        size: Vec<usize>,
        domain: Vec<f64>,
        encode: Vec<f64>,
        decode: Vec<f64>,
        samples: Vec<f64>,
        outs: usize,
    },
    Proc(Obj),
    /// One function per output.
    Each(Vec<Func>),
}

fn nums(o: Option<Obj>) -> Option<Vec<f64>> {
    match o? {
        Obj::Array(a, _) => Some(a.items().iter().filter_map(Obj::num).collect()),
        _ => None,
    }
}

fn pair(v: &[f64], dflt: [f64; 2]) -> [f64; 2] {
    [v.first().copied().unwrap_or(dflt[0]), v.get(1).copied().unwrap_or(dflt[1])]
}

fn parse_fn(ip: &mut Interp, o: &Obj, depth: usize) -> R<Func> {
    if depth > 8 {
        return err("limitcheck");
    }
    let d = match o {
        Obj::Dict(d) => d.clone(),
        Obj::Array(a, false) => return a.items().iter().map(|f| parse_fn(ip, f, depth + 1)).collect::<R<Vec<_>>>().map(Func::Each),
        p @ Obj::Array(_, true) => return Ok(Func::Proc(p.clone())),
        _ => return err("typecheck"),
    };
    let domain = nums(d.get_name(b"Domain")).unwrap_or_else(|| vec![0.0, 1.0]);
    match d.get_name(b"FunctionType").and_then(|o| o.num()).unwrap_or(-1.0) as i64 {
        2 => Ok(Func::Exp {
            c0: nums(d.get_name(b"C0")).unwrap_or_else(|| vec![0.0]),
            c1: nums(d.get_name(b"C1")).unwrap_or_else(|| vec![1.0]),
            n: d.get_name(b"N").and_then(|o| o.num()).unwrap_or(1.0),
            domain: pair(&domain, [0.0, 1.0]),
        }),
        3 => {
            let fns = match d.get_name(b"Functions") {
                Some(Obj::Array(a, _)) => a.items().iter().map(|f| parse_fn(ip, f, depth + 1)).collect::<R<Vec<_>>>()?,
                _ => return err("typecheck"),
            };
            Ok(Func::Stitch {
                fns,
                bounds: nums(d.get_name(b"Bounds")).unwrap_or_default(),
                encode: nums(d.get_name(b"Encode")).unwrap_or_default(),
                domain: pair(&domain, [0.0, 1.0]),
            })
        }
        0 => {
            let size: Vec<usize> = nums(d.get_name(b"Size")).unwrap_or_default().iter().map(|v| v.max(1.0) as usize).collect();
            let range = nums(d.get_name(b"Range")).unwrap_or_default();
            let outs = range.len() / 2;
            let bps = d.get_name(b"BitsPerSample").and_then(|o| o.num()).unwrap_or(8.0) as usize;
            let count = size.iter().try_fold(outs.max(1), |a, s| a.checked_mul(*s)).filter(|n| *n <= MAX_SAMPLES).ok_or(PsErr::Ps("limitcheck"))?;
            // At most four inputs: interpolation visits 2^inputs corners per value.
            if size.is_empty() || size.len() > MAX_INPUTS || outs == 0 || !matches!(bps, 1 | 2 | 4 | 8 | 12 | 16 | 24 | 32) {
                return err("rangecheck");
            }
            ip.charge(count as u64)?;
            ip.alloc(count.saturating_mul(8))?;
            let bytes_needed = (count * bps).div_ceil(8);
            let data = match d.get_name(b"DataSource") {
                Some(Obj::Str(s, _)) => s.bytes(),
                Some(Obj::File(id)) => {
                    let mut v = Vec::with_capacity(bytes_needed);
                    while v.len() < bytes_needed {
                        match ip.srcs.read(id) {
                            Some(c) => v.push(c),
                            None => break,
                        }
                    }
                    v
                }
                _ => return err("typecheck"),
            };
            let max = ((1u64 << bps) - 1) as f64;
            let mut samples = Vec::with_capacity(count);
            let mut bit = 0usize;
            for _ in 0..count {
                let mut v = 0u64;
                for _ in 0..bps {
                    let byte = data.get(bit / 8).copied().unwrap_or(0);
                    v = v << 1 | u64::from(byte >> (7 - bit % 8) & 1);
                    bit += 1;
                }
                samples.push(v as f64 / max);
            }
            let encode = nums(d.get_name(b"Encode")).unwrap_or_else(|| size.iter().flat_map(|s| [0.0, (*s as f64 - 1.0).max(0.0)]).collect());
            let decode = nums(d.get_name(b"Decode")).unwrap_or(range);
            Ok(Func::Sampled { size, domain, encode, decode, samples, outs })
        }
        _ => err("rangecheck"),
    }
}

pub fn eval(ip: &mut Interp, f: &Func, x: &[f64], depth: usize) -> R<Vec<f64>> {
    if depth > 16 {
        return err("limitcheck");
    }
    Ok(match f {
        Func::Exp { c0, c1, n, domain } => {
            let t = x.first().copied().unwrap_or(0.0).clamp(domain[0].min(domain[1]), domain[0].max(domain[1]));
            let k = t.powf(*n);
            let len = c0.len().max(c1.len());
            (0..len)
                .map(|i| c0.get(i).copied().unwrap_or(0.0) + k * (c1.get(i).copied().unwrap_or(1.0) - c0.get(i).copied().unwrap_or(0.0)))
                .collect()
        }
        Func::Stitch { fns, bounds, encode, domain } => {
            let t = x.first().copied().unwrap_or(0.0).clamp(domain[0], domain[1]);
            let i = bounds.iter().take_while(|b| t >= **b).count().min(fns.len().saturating_sub(1));
            let lo = if i == 0 { domain[0] } else { bounds.get(i - 1).copied().unwrap_or(domain[0]) };
            let hi = bounds.get(i).copied().unwrap_or(domain[1]);
            let (e0, e1) = (encode.get(2 * i).copied().unwrap_or(0.0), encode.get(2 * i + 1).copied().unwrap_or(1.0));
            let u = if hi > lo { e0 + (t - lo) / (hi - lo) * (e1 - e0) } else { e0 };
            match fns.get(i) {
                Some(sub) => eval(ip, sub, &[u], depth + 1)?,
                None => vec![0.0],
            }
        }
        Func::Sampled { size, domain, encode, decode, samples, outs } => {
            // Multilinear interpolation between the samples around the point.
            let m = size.len();
            let mut base = 0usize;
            let mut stride = *outs;
            let mut corners: Vec<(usize, f64)> = vec![(0, 1.0)];
            for k in 0..m {
                let (d0, d1) = (domain.get(2 * k).copied().unwrap_or(0.0), domain.get(2 * k + 1).copied().unwrap_or(1.0));
                let s = size.get(k).copied().unwrap_or(1);
                let xk = x.get(k).copied().unwrap_or(0.0).clamp(d0.min(d1), d0.max(d1));
                let (e0, e1) = (encode.get(2 * k).copied().unwrap_or(0.0), encode.get(2 * k + 1).copied().unwrap_or((s - 1) as f64));
                let e = if d1 != d0 { e0 + (xk - d0) / (d1 - d0) * (e1 - e0) } else { e0 };
                let e = e.clamp(0.0, (s - 1) as f64);
                let i0 = e.floor() as usize;
                let i1 = (i0 + 1).min(s - 1);
                let frac = e - i0 as f64;
                base += i0 * stride;
                let step = (i1 - i0) * stride;
                corners = corners.into_iter().flat_map(|(off, w)| [(off, w * (1.0 - frac)), (off + step, w * frac)]).collect();
                stride *= s;
            }
            (0..*outs)
                .map(|j| {
                    let v: f64 = corners.iter().map(|(off, w)| samples.get(base + off + j).copied().unwrap_or(0.0) * w).sum();
                    let (d0, d1) = (decode.get(2 * j).copied().unwrap_or(0.0), decode.get(2 * j + 1).copied().unwrap_or(1.0));
                    d0 + v * (d1 - d0)
                })
                .collect()
        }
        Func::Proc(p) => {
            for v in x {
                ip.push(Obj::Real(*v))?;
            }
            let before = ip.o.len() - x.len();
            ip.call(p.clone())?;
            let n = ip.o.len().saturating_sub(before);
            let mut out = vec![0.0; n];
            for slot in out.iter_mut().rev() {
                *slot = ip.pop_num()?;
            }
            out
        }
        Func::Each(fns) => {
            let mut out = Vec::with_capacity(fns.len());
            for f in fns {
                out.push(eval(ip, f, x, depth + 1)?.first().copied().unwrap_or(0.0));
            }
            out
        }
    })
}

/// Device colours as bytes in one output space (CMYK when every colour is CMYK).
pub fn pack(colors: &[DevColor]) -> (OutSpace, Vec<u8>) {
    let b = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let all_cmyk = colors.iter().all(|c| matches!(c, DevColor::Cmyk(..)));
    let all_gray = colors.iter().all(|c| matches!(c, DevColor::Gray(..)));
    let mut out = Vec::new();
    for c in colors {
        match (*c, all_cmyk, all_gray) {
            (DevColor::Cmyk(c, m, y, k), true, _) => out.extend([b(c), b(m), b(y), b(k)]),
            (DevColor::Gray(g), _, true) => out.push(b(g)),
            (DevColor::Gray(g), _, _) => out.extend([b(g); 3]),
            (DevColor::Rgb(r, g, bl), _, _) => out.extend([b(r), b(g), b(bl)]),
            (DevColor::Cmyk(c, m, y, k), _, _) => out.extend([b(1.0 - (c + k).min(1.0)), b(1.0 - (m + k).min(1.0)), b(1.0 - (y + k).min(1.0))]),
        }
    }
    let space = if all_cmyk {
        OutSpace::Cmyk
    } else if all_gray {
        OutSpace::Gray
    } else {
        OutSpace::Rgb
    };
    (space, out)
}

/// A shading dictionary as the PDF will draw it, or None for kinds not drawn (with a warning).
pub fn build(ip: &mut Interp, d: &PDict) -> R<Option<Shading>> {
    let kind = d.get_name(b"ShadingType").and_then(|o| o.num()).unwrap_or(0.0) as i64;
    let space_obj = d.get_name(b"ColorSpace").ok_or(PsErr::Ps("undefined"))?;
    let space = gfx::parse_space(ip, &space_obj, 0)?;
    let func = match d.get_name(b"Function") {
        Some(f) => Some(parse_fn(ip, &f, 0)?),
        None => None,
    };
    let bbox = nums(d.get_name(b"BBox")).filter(|b| b.len() == 4).map(|b| [b[0], b[1], b[2], b[3]]);
    let extend = match d.get_name(b"Extend") {
        Some(Obj::Array(a, _)) => [matches!(a.get(0), Some(Obj::Bool(true))), matches!(a.get(1), Some(Obj::Bool(true)))],
        _ => [false, false],
    };
    let color_at = |ip: &mut Interp, x: &[f64]| -> R<DevColor> {
        let comps = match &func {
            Some(f) => eval(ip, f, x, 0)?,
            None => x.to_vec(),
        };
        gfx::to_device(ip, &space, &comps, 0)
    };
    let shade = match kind {
        2 | 3 => {
            let coords = nums(d.get_name(b"Coords")).unwrap_or_default();
            let domain = pair(&nums(d.get_name(b"Domain")).unwrap_or_default(), [0.0, 1.0]);
            let mut colors = Vec::with_capacity(SAMPLES);
            for i in 0..SAMPLES {
                let t = domain[0] + (domain[1] - domain[0]) * i as f64 / (SAMPLES - 1) as f64;
                colors.push(color_at(ip, &[t])?);
            }
            let k = if kind == 2 {
                let [a, b, c, e] = <[f64; 4]>::try_from(coords).map_err(|_| PsErr::Ps("rangecheck"))?;
                ShadeKind::Axial([a, b, c, e])
            } else {
                let [a, b, c, e, f, g] = <[f64; 6]>::try_from(coords).map_err(|_| PsErr::Ps("rangecheck"))?;
                ShadeKind::Radial([a, b, c, e, f, g])
            };
            let (space, data) = pack(&colors);
            Shading { kind: k, extend, space, data, samples: SAMPLES, bbox, points: Vec::new() }
        }
        1 => {
            let dom = nums(d.get_name(b"Domain")).unwrap_or_else(|| vec![0.0, 1.0, 0.0, 1.0]);
            let [x0, x1, y0, y1] = <[f64; 4]>::try_from(dom).map_err(|_| PsErr::Ps("rangecheck"))?;
            let matrix = match d.get_name(b"Matrix") {
                Some(Obj::Array(a, _)) => matrix_of(&a)?,
                _ => M::ID,
            };
            let mut colors = Vec::with_capacity(GRID * GRID);
            for j in 0..GRID {
                for i in 0..GRID {
                    let x = x0 + (x1 - x0) * i as f64 / (GRID - 1) as f64;
                    let y = y0 + (y1 - y0) * j as f64 / (GRID - 1) as f64;
                    colors.push(color_at(ip, &[x, y])?);
                }
            }
            let (space, data) = pack(&colors);
            Shading { kind: ShadeKind::Function { domain: [x0, x1, y0, y1], matrix }, extend, space, data, samples: GRID, bbox, points: Vec::new() }
        }
        4..=7 => match crate::ops::mesh::build(ip, d, kind, &space, func.as_ref())? {
            Some(s) => s,
            None => return Ok(None),
        },
        _ => {
            ip.warnings.push(format!("shading type {kind} not drawn"));
            return Ok(None);
        }
    };
    Ok(Some(shade))
}

pub fn shfill(ip: &mut Interp) -> R {
    let d = ip.pop_dict()?;
    if ip.gs.nulldev {
        return Ok(());
    }
    let Some(s) = build(ip, &d)? else { return Ok(()) };
    let ctm = ip.gs.ctm;
    ip.out.shade(&ip.gs.clip, None, false, &ctm, s).map_err(PsErr::Fatal)
}

/// Fill `path` with the current shading pattern; false when the pattern isn't a shading.
pub fn fill_pattern(ip: &mut Interp, path: &[Seg], eo: bool) -> R<bool> {
    let Some(p) = ip.gs.pattern.clone() else { return Ok(false) };
    if p.get_name(b"PatternType").and_then(|o| o.num()) == Some(1.0) {
        return tiling(ip, &p, path, eo).map(|()| true);
    }
    if p.get_name(b"PatternType").and_then(|o| o.num()) != Some(2.0) {
        return Ok(false);
    }
    let Some(Obj::Dict(sh)) = p.get_name(b"Shading") else { return Ok(false) };
    let m = match p.get_name(b"DesignCraftMatrix") {
        Some(Obj::Array(a, _)) => matrix_of(&a)?,
        _ => M::ID,
    };
    let Some(s) = build(ip, &sh)? else { return Ok(true) };
    ip.out.shade(&ip.gs.clip, Some(path), eo, &m, s).map_err(PsErr::Fatal)?;
    Ok(true)
}

/// Fill with a tiling pattern: its cell painted once (per colour, for an uncoloured one) into a
/// PDF tiling pattern, cached on the pattern.
fn tiling(ip: &mut Interp, p: &PDict, path: &[Seg], eo: bool) -> R {
    let uncolored = p.get_name(b"PaintType").and_then(|o| o.num()) == Some(2.0);
    let color = if uncolored { Some(ip.gs.color.clone()) } else { None };
    let key = (crate::obj::Obj::Dict(p.clone()).key(), color.clone().unwrap_or_default().iter().map(|v| v.to_bits()).collect::<Vec<_>>());
    let n = match ip.cells.get(&key) {
        Some(n) => *n,
        None => {
            let n = cell(ip, p, color)?;
            if ip.cells.len() < 10_000 {
                ip.cells.insert(key, n);
            }
            n
        }
    };
    ip.out.fill_tiling(&ip.gs.clip, path, eo, n).map_err(PsErr::Fatal)
}

fn cell(ip: &mut Interp, p: &PDict, color: Option<Vec<f64>>) -> R<usize> {
    let bbox = nums(p.get_name(b"BBox")).filter(|b| b.len() == 4).map(|b| [b[0], b[1], b[2], b[3]]).ok_or(PsErr::Ps("rangecheck"))?;
    let num = |k: &[u8]| p.get_name(k).and_then(|o| o.num());
    let step = (num(b"XStep").unwrap_or(bbox[2] - bbox[0]), num(b"YStep").unwrap_or(bbox[3] - bbox[1]));
    if step.0 == 0.0 || step.1 == 0.0 || !step.0.is_finite() || !step.1.is_finite() {
        return err("rangecheck");
    }
    let m = match p.get_name(b"DesignCraftMatrix") {
        Some(Obj::Array(a, _)) => matrix_of(&a)?,
        _ => M::ID,
    };
    let paint = p.get_name(b"PaintProc").ok_or(PsErr::Ps("undefined"))?;
    let under = ip.gs.pattern_base.as_deref().cloned();
    let cap = ip.out.begin_cell();
    ip.gsave()?;
    // The cell is drawn in pattern space, unclipped, from a fresh path.
    ip.gs.ctm = M::ID;
    ip.gs.clip = None;
    ip.gs.path.clear();
    ip.gs.cur = None;
    ip.gs.nulldev = false;
    if let Some(c) = color {
        // An uncoloured cell paints in the colour given with the pattern.
        ip.gs.space = under.unwrap_or(match c.len() {
            3 => crate::gstate::Space::Rgb,
            4 => crate::gstate::Space::Cmyk,
            _ => crate::gstate::Space::Gray,
        });
        ip.gs.color = c;
        ip.gs.pattern = None;
        ip.gs.color_locked = true;
    }
    let r = ip.push(Obj::Dict(p.clone())).and_then(|()| ip.call(paint));
    ip.grestore();
    let n = ip.out.end_cell(cap, bbox, step, m).map_err(PsErr::Fatal)?;
    r.map(|()| n)
}
