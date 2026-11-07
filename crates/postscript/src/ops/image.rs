//! Sampled images: `image`, `colorimage`, `imagemask`.

use crate::gstate::{DevColor, M, Space};
use crate::interp::{Interp, PsErr, R, err, matrix_of};
use crate::obj::{Obj, PDict};
use crate::ops::gfx;
use crate::pdf::{Image, ImgSpace};

/// The most samples one image may hold.
const MAX_SAMPLES: usize = 400_000_000;

/// Where samples come from: a string (reused), a file, or a procedure (called for more).
fn read_source(ip: &mut Interp, src: &Obj, need: usize) -> R<(Vec<u8>, bool)> {
    let mut out = Vec::new();
    match src {
        Obj::Str(s, _) => {
            let b = s.bytes();
            if b.is_empty() {
                return Ok((vec![0; need], false));
            }
            while out.len() < need {
                out.extend_from_slice(&b);
            }
        }
        Obj::File(id) => {
            if let Some(raw) = ip.srcs.dct(*id) {
                return Ok((raw.to_vec(), true));
            }
            while out.len() < need {
                match ip.srcs.read(*id) {
                    Some(c) => out.push(c),
                    None => break,
                }
            }
            // A filter is read to its end, as printers do: its end marker is part of the data.
            if ip.srcs.is_filter(*id) {
                ip.srcs.drain(*id);
            }
        }
        p @ Obj::Array(_, true) => {
            let mut empty_runs = 0;
            while out.len() < need {
                ip.call(p.clone())?;
                let s = ip.pop_str()?;
                let b = s.bytes();
                if b.is_empty() {
                    empty_runs += 1;
                    if empty_runs > 1 || out.is_empty() {
                        break;
                    }
                    continue;
                }
                out.extend_from_slice(&b);
            }
        }
        _ => return err("typecheck"),
    }
    out.resize(need, 0);
    Ok((out, false))
}

struct Params {
    w: usize,
    h: usize,
    bpc: usize,
    matrix: M,
    sources: Vec<Obj>,
    decode: Option<Vec<f64>>,
    interpolate: bool,
}

fn dict_params(ip: &mut Interp, d: &PDict, comps: usize) -> R<Params> {
    let num = |k: &[u8]| d.get_name(k).and_then(|o| o.num());
    let w = num(b"Width").ok_or(PsErr::Ps("typecheck"))?.max(0.0) as usize;
    let h = num(b"Height").ok_or(PsErr::Ps("typecheck"))?.max(0.0) as usize;
    let bpc = num(b"BitsPerComponent").unwrap_or(8.0) as usize;
    let matrix = match d.get_name(b"ImageMatrix") {
        Some(Obj::Array(a, _)) => matrix_of(&a)?,
        _ => return err("typecheck"),
    };
    let multi = matches!(d.get_name(b"MultipleDataSources"), Some(Obj::Bool(true)));
    let sources = match d.get_name(b"DataSource").ok_or(PsErr::Ps("typecheck"))? {
        Obj::Array(a, false) if multi => a.items(),
        o => vec![o],
    };
    if multi && sources.len() < comps {
        return err("rangecheck");
    }
    let decode = match d.get_name(b"Decode") {
        Some(Obj::Array(a, _)) => Some(a.items().iter().filter_map(Obj::num).collect()),
        _ => None,
    };
    let interpolate = matches!(d.get_name(b"Interpolate"), Some(Obj::Bool(true)));
    let _ = ip;
    Ok(Params { w, h, bpc, matrix, sources, decode, interpolate })
}

/// Read all samples, pixel-interleaved (`comps` per pixel), rows padded to bytes.
fn samples(ip: &mut Interp, p: &Params, comps: usize) -> R<(Vec<u8>, bool)> {
    let total = p.w.checked_mul(p.h).and_then(|n| n.checked_mul(comps)).ok_or(PsErr::Ps("limitcheck"))?;
    if total > MAX_SAMPLES || !matches!(p.bpc, 1 | 2 | 4 | 8 | 12 | 16) {
        return err("limitcheck");
    }
    ip.alloc(total)?;
    ip.charge_bytes(total)?;
    if p.sources.len() <= 1 {
        let row = (p.w * comps * p.bpc).div_ceil(8);
        let src = p.sources.first().cloned().ok_or(PsErr::Ps("typecheck"))?;
        return read_source(ip, &src, row * p.h);
    }
    // One source per component, read in turn (each call or string gives the next piece of its
    // component, as Photoshop's row-by-row procedures expect), then interleaved (8-bit only).
    if p.bpc != 8 {
        return err("limitcheck");
    }
    let plane = p.w * p.h;
    let mut planes: Vec<Vec<u8>> = vec![Vec::new(); comps];
    let sources: Vec<Obj> = p.sources.iter().take(comps).cloned().collect();
    let mut stalled = 0;
    while planes.iter().any(|pl| pl.len() < plane) && stalled < 2 {
        let mut progress = false;
        for (pl, src) in planes.iter_mut().zip(&sources) {
            if pl.len() >= plane {
                continue;
            }
            let piece = match src {
                p @ Obj::Array(_, true) => {
                    ip.call(p.clone())?;
                    ip.pop_str()?.bytes()
                }
                Obj::Str(s, _) => s.bytes(),
                Obj::File(id) => {
                    let row = p.w.max(1);
                    let mut v = Vec::with_capacity(row);
                    while v.len() < row {
                        match ip.srcs.read(*id) {
                            Some(c) => v.push(c),
                            None => break,
                        }
                    }
                    v
                }
                _ => return err("typecheck"),
            };
            progress |= !piece.is_empty();
            pl.extend_from_slice(&piece);
        }
        stalled = if progress { 0 } else { stalled + 1 };
    }
    for pl in &mut planes {
        pl.resize(plane, 0);
    }
    let mut out = Vec::with_capacity(total);
    for i in 0..plane {
        for pl in &planes {
            out.push(pl.get(i).copied().unwrap_or(0));
        }
    }
    Ok((out, false))
}

/// The unit square → page matrix for an image of w × h samples.
fn placement(ip: &Interp, p: &Params) -> R<M> {
    let inv = p.matrix.inv().ok_or(PsErr::Ps("undefinedresult"))?;
    let s = M([p.w as f64, 0.0, 0.0, -(p.h as f64), 0.0, p.h as f64]);
    Ok(s.then(&inv).then(&ip.gs.ctm))
}

/// 12- and 16-bit samples as 8-bit.
fn to_8bit(data: Vec<u8>, bpc: usize) -> Vec<u8> {
    match bpc {
        16 => data.chunks(2).map(|c| c.first().copied().unwrap_or(0)).collect(),
        12 => {
            let mut out = Vec::with_capacity(data.len() * 2 / 3);
            for c in data.chunks(3) {
                let (a, b, d) = (c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0), c.get(2).copied().unwrap_or(0));
                out.push(a);
                out.push((b << 4) | (d >> 4));
            }
            out
        }
        _ => data,
    }
}

fn paint(ip: &mut Interp, p: Params, space: Space) -> R {
    let comps = space.comps();
    let (data, dct) = samples(ip, &p, comps)?;
    let cm = placement(ip, &p)?;
    if ip.gs.nulldev {
        return Ok(());
    }
    let (data, bpc) = if !dct && matches!(p.bpc, 12 | 16) { (to_8bit(data, p.bpc), 8) } else { (data, p.bpc) };
    let decode = p.decode.map(|d| d.iter().map(|v| *v as f32).collect::<Vec<_>>());
    let img_space = match &space {
        Space::Gray => Some(ImgSpace::Gray),
        Space::Rgb => Some(ImgSpace::Rgb),
        Space::Cmyk => Some(ImgSpace::Cmyk),
        Space::Indexed { base, hival, lookup } if matches!(**base, Space::Gray | Space::Rgb | Space::Cmyk) => Some(ImgSpace::Indexed {
            base: Box::new(match **base {
                Space::Rgb => ImgSpace::Rgb,
                Space::Cmyk => ImgSpace::Cmyk,
                _ => ImgSpace::Gray,
            }),
            hival: u8::try_from(*hival).unwrap_or(255),
            lookup: lookup.clone(),
        }),
        _ => None,
    };
    if let Some(space) = img_space {
        let img =
            Image { w: p.w as u32, h: p.h as u32, bpc: bpc as u8, space: Some(space), decode, data, dct, interpolate: p.interpolate, smask: None };
        return ip.out.image(&ip.gs.clip, &cm, img, None).map_err(PsErr::Fatal);
    }
    // Other spaces (Separation, DeviceN, Indexed over them, Lab): convert each pixel, through a
    // table of the 256 values when there is one component.
    if dct || bpc != 8 {
        ip.warnings.push("image in an unsupported colour space skipped".into());
        return Ok(());
    }
    // Converting each pixel: work and the converted copy.
    ip.charge(data.len() as u64 / 4 + 1)?;
    ip.alloc(p.w.saturating_mul(p.h).saturating_mul(4))?;
    let mut rgb = Vec::with_capacity(p.w * p.h * 4);
    if comps == 1 {
        let mut table = Vec::with_capacity(256);
        let dmax = match &space {
            Space::Indexed { hival, .. } => *hival as f64,
            _ => 1.0,
        };
        for v in 0..256 {
            let x = f64::from(v) / 255.0 * dmax;
            table.push(gfx::to_device(ip, &space, &[x], 0)?);
        }
        let cmyk = table.iter().any(|c| matches!(c, DevColor::Cmyk(..)));
        for s in &data {
            push_color(&mut rgb, table.get(usize::from(*s)).copied().unwrap_or(DevColor::Gray(0.0)), cmyk);
        }
        let space = if cmyk { ImgSpace::Cmyk } else { ImgSpace::Rgb };
        let img = Image {
            w: p.w as u32,
            h: p.h as u32,
            bpc: 8,
            space: Some(space),
            decode: None,
            data: rgb,
            dct: false,
            interpolate: p.interpolate,
            smask: None,
        };
        return ip.out.image(&ip.gs.clip, &cm, img, None).map_err(PsErr::Fatal);
    }
    if p.w * p.h > 2_000_000 {
        ip.warnings.push("multi-component image in a special colour space too large to convert".into());
        return Ok(());
    }
    let mut cache: std::collections::HashMap<Vec<u8>, DevColor> = std::collections::HashMap::new();
    let mut colors = Vec::with_capacity(p.w * p.h);
    for px in data.chunks(comps) {
        let c = match cache.get(px) {
            Some(c) => *c,
            None => {
                let v: Vec<f64> = px.iter().map(|b| f64::from(*b) / 255.0).collect();
                let c = gfx::to_device(ip, &space, &v, 0)?;
                if cache.len() < 65536 {
                    cache.insert(px.to_vec(), c);
                }
                c
            }
        };
        colors.push(c);
    }
    let cmyk = colors.iter().any(|c| matches!(c, DevColor::Cmyk(..)));
    for c in colors {
        push_color(&mut rgb, c, cmyk);
    }
    let space = if cmyk { ImgSpace::Cmyk } else { ImgSpace::Rgb };
    let img = Image {
        w: p.w as u32,
        h: p.h as u32,
        bpc: 8,
        space: Some(space),
        decode: None,
        data: rgb,
        dct: false,
        interpolate: p.interpolate,
        smask: None,
    };
    ip.out.image(&ip.gs.clip, &cm, img, None).map_err(PsErr::Fatal)
}

fn push_color(out: &mut Vec<u8>, c: DevColor, cmyk: bool) {
    let b = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    match (c, cmyk) {
        (DevColor::Cmyk(c, m, y, k), true) => out.extend([b(c), b(m), b(y), b(k)]),
        (DevColor::Gray(g), true) => out.extend([0, 0, 0, b(1.0 - g)]),
        (DevColor::Rgb(r, g, bl), true) => out.extend([b(1.0 - r), b(1.0 - g), b(1.0 - bl), 0]),
        (DevColor::Gray(g), false) => out.extend([b(g); 3]),
        (DevColor::Rgb(r, g, bl), false) => out.extend([b(r), b(g), b(bl)]),
        (DevColor::Cmyk(c, m, y, k), false) => out.extend([b(1.0 - (c + k).min(1.0)), b(1.0 - (m + k).min(1.0)), b(1.0 - (y + k).min(1.0))]),
    }
}

pub fn image(ip: &mut Interp) -> R {
    if let Obj::Dict(d) = ip.top(0)?.clone() {
        ip.pop()?;
        let space = ip.gs.space.clone();
        let p = dict_params(ip, &d, space.comps())?;
        return paint(ip, p, space);
    }
    let src = ip.pop()?;
    let matrix = ip.pop_matrix()?;
    let bpc = ip.pop_usize()?;
    let h = ip.pop_usize()?;
    let w = ip.pop_usize()?;
    paint(ip, Params { w, h, bpc, matrix, sources: vec![src], decode: None, interpolate: false }, Space::Gray)
}

pub fn colorimage(ip: &mut Interp) -> R {
    let ncomp = ip.pop_usize()?;
    let multi = ip.pop_bool()?;
    let space = match ncomp {
        1 => Space::Gray,
        3 => Space::Rgb,
        4 => Space::Cmyk,
        _ => return err("rangecheck"),
    };
    let mut sources = Vec::new();
    for _ in 0..if multi { ncomp } else { 1 } {
        sources.push(ip.pop()?);
    }
    sources.reverse();
    let matrix = ip.pop_matrix()?;
    let bpc = ip.pop_usize()?;
    let h = ip.pop_usize()?;
    let w = ip.pop_usize()?;
    paint(ip, Params { w, h, bpc, matrix, sources, decode: None, interpolate: false }, space)
}

pub fn imagemask(ip: &mut Interp) -> R {
    let (p, polarity) = if let Obj::Dict(d) = ip.top(0)?.clone() {
        ip.pop()?;
        let mut p = dict_params(ip, &d, 1)?;
        let inverted = p.decode.as_ref().and_then(|d| d.first()).is_some_and(|v| *v > 0.5);
        p.decode = None;
        (p, inverted)
    } else {
        let src = ip.pop()?;
        let matrix = ip.pop_matrix()?;
        let polarity = ip.pop_bool()?;
        let h = ip.pop_usize()?;
        let w = ip.pop_usize()?;
        (Params { w, h, bpc: 1, matrix, sources: vec![src], decode: None, interpolate: false }, polarity)
    };
    let p = Params { bpc: 1, ..p };
    let (data, _) = samples(ip, &p, 1)?;
    let cm = placement(ip, &p)?;
    if ip.gs.nulldev {
        return Ok(());
    }
    let color = gfx::current_color(ip)?;
    // PDF stencil masks paint 0 bits; polarity true paints the 1 bits.
    let decode = if polarity { Some(vec![1.0, 0.0]) } else { None };
    let img = Image { w: p.w as u32, h: p.h as u32, bpc: 1, space: None, decode, data, dct: false, interpolate: false, smask: None };
    ip.out.image(&ip.gs.clip, &cm, img, Some(color)).map_err(PsErr::Fatal)
}
