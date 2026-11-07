//! Graphics: the graphics state, matrices, paths, painting, clipping and colour.

use std::rc::Rc;

use crate::gstate::{Clip, DevColor, GState, M, MAX_PATH, Seg, Space, arc_curves, bbox, flatten};
use crate::interp::{Interp, PsErr, R, err, matrix_obj};
use crate::obj::{Obj, PArr, PDict};
use crate::pdf::StrokeParams;

// ----- graphics-state stack -----

pub fn gsave(ip: &mut Interp) -> R {
    ip.gsave()
}

pub fn grestore(ip: &mut Interp) -> R {
    ip.grestore();
    Ok(())
}

pub fn grestoreall(ip: &mut Interp) -> R {
    let floor = ip.saves.last().copied().unwrap_or(0);
    while ip.gstack.len() > floor {
        ip.grestore();
    }
    Ok(())
}

pub fn initgraphics(ip: &mut Interp) -> R {
    let font = ip.gs.font.clone();
    ip.gs = GState { font, ..GState::default() };
    Ok(())
}

pub fn save(ip: &mut Interp) -> R {
    ip.gsave()?;
    ip.saves.push(ip.gstack.len());
    let n = ip.saves.len();
    ip.push(Obj::Save(n))
}

pub fn restore(ip: &mut Interp) -> R {
    let Obj::Save(n) = ip.pop()? else { return err("typecheck") };
    if n == 0 || n > ip.saves.len() {
        return err("invalidrestore");
    }
    let depth = ip.saves.get(n - 1).copied().unwrap_or(0);
    ip.saves.truncate(n - 1);
    // Back to the state saved with it (save did a gsave, leaving `depth` states stacked).
    while ip.gstack.len() >= depth.max(1) {
        let Some(g) = ip.gstack.pop() else { break };
        ip.gs = g;
    }
    Ok(())
}

pub fn gstate(ip: &mut Interp) -> R {
    ip.charge_gstate()?;
    if ip.gstates.len() > 100_000 {
        return err("limitcheck");
    }
    ip.gstates.push(ip.gs.clone());
    let n = ip.gstates.len() - 1;
    ip.push(Obj::GState(n))
}

pub fn currentgstate(ip: &mut Interp) -> R {
    ip.charge_gstate()?;
    let Obj::GState(n) = ip.pop()? else { return err("typecheck") };
    if let Some(slot) = ip.gstates.get_mut(n) {
        *slot = ip.gs.clone();
    }
    ip.push(Obj::GState(n))
}

pub fn setgstate(ip: &mut Interp) -> R {
    ip.charge_gstate()?;
    let Obj::GState(n) = ip.pop()? else { return err("typecheck") };
    if let Some(g) = ip.gstates.get(n) {
        ip.gs = g.clone();
    }
    Ok(())
}

// ----- line parameters -----

pub fn setlinewidth(ip: &mut Interp) -> R {
    ip.gs.line_width = ip.pop_num()?.abs();
    Ok(())
}

pub fn currentlinewidth(ip: &mut Interp) -> R {
    let w = ip.gs.line_width;
    ip.push(Obj::Real(w))
}

pub fn setlinecap(ip: &mut Interp) -> R {
    ip.gs.cap = ip.pop_int()?.clamp(0, 2);
    Ok(())
}

pub fn currentlinecap(ip: &mut Interp) -> R {
    let v = ip.gs.cap;
    ip.push(Obj::Int(v))
}

pub fn setlinejoin(ip: &mut Interp) -> R {
    ip.gs.join = ip.pop_int()?.clamp(0, 2);
    Ok(())
}

pub fn currentlinejoin(ip: &mut Interp) -> R {
    let v = ip.gs.join;
    ip.push(Obj::Int(v))
}

pub fn setmiterlimit(ip: &mut Interp) -> R {
    ip.gs.miter = ip.pop_num()?.max(1.0);
    Ok(())
}

pub fn currentmiterlimit(ip: &mut Interp) -> R {
    let v = ip.gs.miter;
    ip.push(Obj::Real(v))
}

pub fn setdash(ip: &mut Interp) -> R {
    let phase = ip.pop_num()?;
    let a = ip.pop_arr()?;
    let mut v = Vec::with_capacity(a.len.min(64));
    for o in a.items().iter().take(64) {
        v.push(o.num().ok_or(PsErr::Ps("typecheck"))?);
    }
    ip.gs.dash = (v, phase);
    Ok(())
}

pub fn currentdash(ip: &mut Interp) -> R {
    let (v, phase) = ip.gs.dash.clone();
    ip.push(Obj::Array(PArr::new(v.into_iter().map(Obj::Real).collect()), false))?;
    ip.push(Obj::Real(phase))
}

pub fn setflat(ip: &mut Interp) -> R {
    ip.gs.flat = ip.pop_num()?.clamp(0.2, 100.0);
    Ok(())
}

pub fn currentflat(ip: &mut Interp) -> R {
    let v = ip.gs.flat;
    ip.push(Obj::Real(v))
}

pub fn setstrokeadjust(ip: &mut Interp) -> R {
    ip.gs.stroke_adjust = ip.pop_bool()?;
    Ok(())
}

pub fn currentstrokeadjust(ip: &mut Interp) -> R {
    let v = ip.gs.stroke_adjust;
    ip.push(Obj::Bool(v))
}

pub fn setoverprint(ip: &mut Interp) -> R {
    ip.gs.overprint = ip.pop_bool()?;
    Ok(())
}

pub fn currentoverprint(ip: &mut Interp) -> R {
    let v = ip.gs.overprint;
    ip.push(Obj::Bool(v))
}

// ----- matrices -----

fn fill_matrix(a: &PArr, m: &M) -> R {
    if a.len != 6 {
        return err("rangecheck");
    }
    for (i, v) in m.0.iter().enumerate() {
        a.set(i, Obj::Real(*v));
    }
    Ok(())
}

pub fn matrix(ip: &mut Interp) -> R {
    ip.push(matrix_obj(&M::ID))
}

pub fn identmatrix(ip: &mut Interp) -> R {
    let a = ip.pop_arr()?;
    fill_matrix(&a, &M::ID)?;
    ip.push(Obj::Array(a, false))
}

pub fn defaultmatrix(ip: &mut Interp) -> R {
    identmatrix(ip)
}

pub fn initmatrix(ip: &mut Interp) -> R {
    ip.gs.ctm = M::ID;
    Ok(())
}

pub fn currentmatrix(ip: &mut Interp) -> R {
    let a = ip.pop_arr()?;
    fill_matrix(&a, &ip.gs.ctm)?;
    ip.push(Obj::Array(a, false))
}

pub fn setmatrix(ip: &mut Interp) -> R {
    let m = ip.pop_matrix()?;
    if !m.finite() {
        return err("undefinedresult");
    }
    ip.gs.ctm = m;
    Ok(())
}

/// Operators that either change the CTM or, given a matrix operand, fill it in.
fn modify(ip: &mut Interp, args: usize, make: fn(&[f64]) -> M) -> R {
    if let Ok(Obj::Array(a, _)) = ip.top(0).cloned() {
        ip.pop()?;
        let mut v = vec![0.0; args];
        for slot in v.iter_mut().rev() {
            *slot = ip.pop_num()?;
        }
        fill_matrix(&a, &make(&v))?;
        return ip.push(Obj::Array(a, false));
    }
    let mut v = vec![0.0; args];
    for slot in v.iter_mut().rev() {
        *slot = ip.pop_num()?;
    }
    let m = make(&v).then(&ip.gs.ctm);
    if !m.finite() {
        return err("undefinedresult");
    }
    ip.gs.ctm = m;
    Ok(())
}

fn arg(v: &[f64], i: usize) -> f64 {
    v.get(i).copied().unwrap_or(0.0)
}

pub fn translate(ip: &mut Interp) -> R {
    modify(ip, 2, |v| M([1.0, 0.0, 0.0, 1.0, arg(v, 0), arg(v, 1)]))
}

pub fn scale(ip: &mut Interp) -> R {
    modify(ip, 2, |v| M([arg(v, 0), 0.0, 0.0, arg(v, 1), 0.0, 0.0]))
}

pub fn rotate(ip: &mut Interp) -> R {
    modify(ip, 1, |v| {
        let a = arg(v, 0).to_radians();
        let (s, c) = a.sin_cos();
        M([c, s, -s, c, 0.0, 0.0])
    })
}

pub fn concat(ip: &mut Interp) -> R {
    let m = ip.pop_matrix()?;
    let r = m.then(&ip.gs.ctm);
    if !r.finite() {
        return err("undefinedresult");
    }
    ip.gs.ctm = r;
    Ok(())
}

pub fn concatmatrix(ip: &mut Interp) -> R {
    let dst = ip.pop_arr()?;
    let m2 = ip.pop_matrix()?;
    let m1 = ip.pop_matrix()?;
    fill_matrix(&dst, &m1.then(&m2))?;
    ip.push(Obj::Array(dst, false))
}

pub fn invertmatrix(ip: &mut Interp) -> R {
    let dst = ip.pop_arr()?;
    let m = ip.pop_matrix()?;
    let inv = m.inv().ok_or(PsErr::Ps("undefinedresult"))?;
    fill_matrix(&dst, &inv)?;
    ip.push(Obj::Array(dst, false))
}

fn xform(ip: &mut Interp, f: fn(&M, f64, f64) -> Option<(f64, f64)>) -> R {
    let m = match ip.top(0)? {
        Obj::Array(..) => ip.pop_matrix()?,
        _ => ip.gs.ctm,
    };
    let y = ip.pop_num()?;
    let x = ip.pop_num()?;
    let (x, y) = f(&m, x, y).ok_or(PsErr::Ps("undefinedresult"))?;
    ip.push(Obj::Real(x))?;
    ip.push(Obj::Real(y))
}

pub fn transform(ip: &mut Interp) -> R {
    xform(ip, |m, x, y| Some(m.apply(x, y)))
}

pub fn itransform(ip: &mut Interp) -> R {
    xform(ip, |m, x, y| Some(m.inv()?.apply(x, y)))
}

pub fn dtransform(ip: &mut Interp) -> R {
    xform(ip, |m, x, y| Some(m.delta(x, y)))
}

pub fn idtransform(ip: &mut Interp) -> R {
    xform(ip, |m, x, y| Some(m.inv()?.delta(x, y)))
}

// ----- paths -----

fn add_seg(ip: &mut Interp, s: Seg) -> R {
    if ip.gs.path.len() >= MAX_PATH {
        return err("limitcheck");
    }
    ip.gs.path.push(s);
    Ok(())
}

pub fn newpath(ip: &mut Interp) -> R {
    ip.gs.path.clear();
    ip.gs.cur = None;
    ip.gs.start = None;
    Ok(())
}

fn user_point(ip: &mut Interp) -> R<(f64, f64)> {
    let y = ip.pop_num()?;
    let x = ip.pop_num()?;
    Ok(ip.gs.ctm.apply(x, y))
}

fn rel_point(ip: &mut Interp) -> R<(f64, f64)> {
    let dy = ip.pop_num()?;
    let dx = ip.pop_num()?;
    let (cx, cy) = ip.gs.cur.ok_or(PsErr::Ps("nocurrentpoint"))?;
    let (dx, dy) = ip.gs.ctm.delta(dx, dy);
    Ok((cx + dx, cy + dy))
}

fn move_dev(ip: &mut Interp, p: (f64, f64)) -> R {
    // Consecutive movetos keep only the last.
    if let Some(Seg::Move(..)) = ip.gs.path.last() {
        ip.gs.path.pop();
    }
    add_seg(ip, Seg::Move(p.0, p.1))?;
    ip.gs.cur = Some(p);
    ip.gs.start = Some(p);
    Ok(())
}

fn line_dev(ip: &mut Interp, p: (f64, f64)) -> R {
    if ip.gs.cur.is_none() {
        return err("nocurrentpoint");
    }
    if matches!(ip.gs.path.last(), Some(Seg::Close))
        && let Some(s) = ip.gs.start
    {
        add_seg(ip, Seg::Move(s.0, s.1))?;
    }
    add_seg(ip, Seg::Line(p.0, p.1))?;
    ip.gs.cur = Some(p);
    Ok(())
}

fn curve_dev(ip: &mut Interp, a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> R {
    if ip.gs.cur.is_none() {
        return err("nocurrentpoint");
    }
    if matches!(ip.gs.path.last(), Some(Seg::Close))
        && let Some(s) = ip.gs.start
    {
        add_seg(ip, Seg::Move(s.0, s.1))?;
    }
    add_seg(ip, Seg::Curve(a.0, a.1, b.0, b.1, c.0, c.1))?;
    ip.gs.cur = Some(c);
    Ok(())
}

pub fn moveto(ip: &mut Interp) -> R {
    let p = user_point(ip)?;
    move_dev(ip, p)
}

pub fn rmoveto(ip: &mut Interp) -> R {
    let p = rel_point(ip)?;
    move_dev(ip, p)
}

pub fn lineto(ip: &mut Interp) -> R {
    let p = user_point(ip)?;
    line_dev(ip, p)
}

pub fn rlineto(ip: &mut Interp) -> R {
    let p = rel_point(ip)?;
    line_dev(ip, p)
}

pub fn curveto(ip: &mut Interp) -> R {
    let c = user_point(ip)?;
    let b = user_point(ip)?;
    let a = user_point(ip)?;
    curve_dev(ip, a, b, c)
}

pub fn rcurveto(ip: &mut Interp) -> R {
    let mut v = [0.0; 6];
    for slot in v.iter_mut().rev() {
        *slot = ip.pop_num()?;
    }
    let (cx, cy) = ip.gs.cur.ok_or(PsErr::Ps("nocurrentpoint"))?;
    let m = ip.gs.ctm;
    let d = |x: f64, y: f64| {
        let (dx, dy) = m.delta(x, y);
        (cx + dx, cy + dy)
    };
    curve_dev(ip, d(v[0], v[1]), d(v[2], v[3]), d(v[4], v[5]))
}

pub fn closepath(ip: &mut Interp) -> R {
    if ip.gs.cur.is_some() && !matches!(ip.gs.path.last(), Some(Seg::Close) | None) {
        add_seg(ip, Seg::Close)?;
        ip.gs.cur = ip.gs.start;
    }
    Ok(())
}

fn arc_common(ip: &mut Interp, negative: bool) -> R {
    let mut a1 = ip.pop_num()?;
    let a0 = ip.pop_num()?;
    let r = ip.pop_num()?;
    let cy = ip.pop_num()?;
    let cx = ip.pop_num()?;
    if negative {
        while a1 > a0 {
            a1 -= 360.0;
        }
    } else {
        while a1 < a0 {
            a1 += 360.0;
        }
    }
    let (start, curves) = arc_curves(cx, cy, r, a0, a1);
    let m = ip.gs.ctm;
    let s = m.apply(start.0, start.1);
    if ip.gs.cur.is_some() {
        line_dev(ip, s)?
    } else {
        move_dev(ip, s)?
    }
    for c in curves {
        curve_dev(ip, m.apply(c[0], c[1]), m.apply(c[2], c[3]), m.apply(c[4], c[5]))?;
    }
    Ok(())
}

pub fn arc(ip: &mut Interp) -> R {
    arc_common(ip, false)
}

pub fn arcn(ip: &mut Interp) -> R {
    arc_common(ip, true)
}

/// `arct` and `arcto`: a line toward (x1, y1), then an arc of radius r tangent to both lines.
fn arct_common(ip: &mut Interp, push_points: bool) -> R {
    let r = ip.pop_num()?;
    let y2 = ip.pop_num()?;
    let x2 = ip.pop_num()?;
    let y1 = ip.pop_num()?;
    let x1 = ip.pop_num()?;
    let inv = ip.gs.ctm.inv().ok_or(PsErr::Ps("undefinedresult"))?;
    let (dx0, dy0) = ip.gs.cur.ok_or(PsErr::Ps("nocurrentpoint"))?;
    let (x0, y0) = inv.apply(dx0, dy0);
    let (ux, uy) = (x0 - x1, y0 - y1);
    let (vx, vy) = (x2 - x1, y2 - y1);
    let (lu, lv) = (ux.hypot(uy), vx.hypot(vy));
    let m = ip.gs.ctm;
    if lu == 0.0 || lv == 0.0 || r == 0.0 {
        line_dev(ip, m.apply(x1, y1))?;
        if push_points {
            for v in [x1, y1, x1, y1] {
                ip.push(Obj::Real(v))?;
            }
        }
        return Ok(());
    }
    let cos = ((ux * vx + uy * vy) / (lu * lv)).clamp(-1.0, 1.0);
    let theta = cos.acos();
    let t = r / (theta / 2.0).tan();
    if !t.is_finite() {
        line_dev(ip, m.apply(x1, y1))?;
        return Ok(());
    }
    let (t1x, t1y) = (x1 + ux / lu * t, y1 + uy / lu * t);
    let (t2x, t2y) = (x1 + vx / lv * t, y1 + vy / lv * t);
    line_dev(ip, m.apply(t1x, t1y))?;
    // One cubic for the arc between the tangent points.
    let sweep = std::f64::consts::PI - theta;
    let k = 4.0 / 3.0 * (sweep / 4.0).tan();
    let c1 = (t1x + (x1 - t1x) / t.max(1e-12) * k * r, t1y + (y1 - t1y) / t.max(1e-12) * k * r);
    let c2 = (t2x + (x1 - t2x) / t.max(1e-12) * k * r, t2y + (y1 - t2y) / t.max(1e-12) * k * r);
    curve_dev(ip, m.apply(c1.0, c1.1), m.apply(c2.0, c2.1), m.apply(t2x, t2y))?;
    if push_points {
        for v in [t1x, t1y, t2x, t2y] {
            ip.push(Obj::Real(v))?;
        }
    }
    Ok(())
}

pub fn arct(ip: &mut Interp) -> R {
    arct_common(ip, false)
}

pub fn arcto(ip: &mut Interp) -> R {
    arct_common(ip, true)
}

pub fn currentpoint(ip: &mut Interp) -> R {
    let (x, y) = ip.gs.cur.ok_or(PsErr::Ps("nocurrentpoint"))?;
    let (x, y) = ip.gs.ctm.inv().ok_or(PsErr::Ps("undefinedresult"))?.apply(x, y);
    ip.push(Obj::Real(x))?;
    ip.push(Obj::Real(y))
}

pub fn flattenpath(ip: &mut Interp) -> R {
    ip.charge_path(ip.gs.path.len().saturating_mul(16))?;
    let tol = ip.gs.flat;
    let flat = flatten(&ip.gs.path, tol);
    if flat.len() > MAX_PATH {
        return err("limitcheck");
    }
    ip.gs.path = flat;
    Ok(())
}

pub fn reversepath(ip: &mut Interp) -> R {
    ip.charge_path(ip.gs.path.len())?;
    // Reverse each subpath.
    let mut out = Vec::with_capacity(ip.gs.path.len());
    let mut sub: Vec<Seg> = Vec::new();
    let flush = |sub: &mut Vec<Seg>, out: &mut Vec<Seg>| {
        let closed = matches!(sub.last(), Some(Seg::Close));
        let pts: Vec<(f64, f64, Option<[f64; 4]>)> = sub
            .iter()
            .filter_map(|s| match *s {
                Seg::Move(x, y) | Seg::Line(x, y) => Some((x, y, None)),
                Seg::Curve(a, b, c, d, e, f) => Some((e, f, Some([a, b, c, d]))),
                Seg::Close => None,
            })
            .collect();
        if let Some(last) = pts.last() {
            out.push(Seg::Move(last.0, last.1));
            for i in (1..pts.len()).rev() {
                let (Some(cur), Some(prev)) = (pts.get(i), pts.get(i - 1)) else { continue };
                match cur.2 {
                    Some([a, b, c, d]) => out.push(Seg::Curve(c, d, a, b, prev.0, prev.1)),
                    None => out.push(Seg::Line(prev.0, prev.1)),
                }
            }
            if closed {
                out.push(Seg::Close);
            }
        }
        sub.clear();
    };
    for s in ip.gs.path.clone() {
        if matches!(s, Seg::Move(..)) {
            flush(&mut sub, &mut out);
        }
        sub.push(s);
    }
    flush(&mut sub, &mut out);
    ip.gs.path = out;
    Ok(())
}

/// `strokepath`: the outline of what `stroke` would paint, as the current path. The pen is
/// shaped in user space (width, caps, joins, miter limit, dashes), as for `stroke`.
pub fn strokepath(ip: &mut Interp) -> R {
    ip.charge_path(ip.gs.path.len().saturating_mul(4))?;
    use kurbo::{Cap, Join, PathEl, Point, Stroke, StrokeOpts};
    let ctm = ip.gs.ctm;
    let Some(inv) = ctm.inv() else {
        ip.gs.path.clear();
        return Ok(());
    };
    let p = |x: f64, y: f64| {
        let (x, y) = inv.apply(x, y);
        Point::new(x, y)
    };
    let mut els = Vec::with_capacity(ip.gs.path.len());
    for s in &ip.gs.path {
        els.push(match *s {
            Seg::Move(x, y) => PathEl::MoveTo(p(x, y)),
            Seg::Line(x, y) => PathEl::LineTo(p(x, y)),
            Seg::Curve(a, b, c, d, e, f) => PathEl::CurveTo(p(a, b), p(c, d), p(e, f)),
            Seg::Close => PathEl::ClosePath,
        });
    }
    let g = &ip.gs;
    let cap = match g.cap {
        1 => Cap::Round,
        2 => Cap::Square,
        _ => Cap::Butt,
    };
    let join = match g.join {
        1 => Join::Round,
        2 => Join::Bevel,
        _ => Join::Miter,
    };
    let mut style = Stroke::new(g.line_width.max(1e-3)).with_caps(cap).with_join(join).with_miter_limit(g.miter.max(1.0));
    let user: Vec<Seg> = g.path.iter().map(|s| s.map(&inv)).collect();
    if !user.iter().all(|s| match *s {
        Seg::Move(x, y) | Seg::Line(x, y) => x.is_finite() && y.is_finite(),
        Seg::Curve(a, b, c, d, e, f) => [a, b, c, d, e, f].iter().all(|v| v.is_finite()),
        Seg::Close => true,
    }) {
        ip.gs.path.clear();
        return Ok(());
    }
    if crate::gstate::dashes_ok(&user, &g.dash.0) && g.dash.1.is_finite() {
        style = style.with_dashes(g.dash.1, g.dash.0.clone());
    }
    // Tolerance in user space, about a twentieth of a device unit.
    let scale = (ctm.0[0] * ctm.0[3] - ctm.0[1] * ctm.0[2]).abs().sqrt().max(1e-9);
    let out = kurbo::stroke(els, &style, &StrokeOpts::default(), (0.05 / scale).max(1e-6));
    let mut path = Vec::with_capacity(out.elements().len());
    let mut cur = (0.0, 0.0);
    for el in out.elements() {
        if path.len() >= MAX_PATH {
            return err("limitcheck");
        }
        let d = |q: Point| ctm.apply(q.x, q.y);
        match *el {
            PathEl::MoveTo(q) => {
                cur = d(q);
                path.push(Seg::Move(cur.0, cur.1));
            }
            PathEl::LineTo(q) => {
                cur = d(q);
                path.push(Seg::Line(cur.0, cur.1));
            }
            PathEl::QuadTo(c, q) => {
                let (c, e) = (d(c), d(q));
                let a = (cur.0 + 2.0 / 3.0 * (c.0 - cur.0), cur.1 + 2.0 / 3.0 * (c.1 - cur.1));
                let b = (e.0 + 2.0 / 3.0 * (c.0 - e.0), e.1 + 2.0 / 3.0 * (c.1 - e.1));
                path.push(Seg::Curve(a.0, a.1, b.0, b.1, e.0, e.1));
                cur = e;
            }
            PathEl::CurveTo(a, b, q) => {
                let (a, b, e) = (d(a), d(b), d(q));
                path.push(Seg::Curve(a.0, a.1, b.0, b.1, e.0, e.1));
                cur = e;
            }
            PathEl::ClosePath => path.push(Seg::Close),
        }
    }
    ip.gs.path = path;
    ip.gs.cur = Some(cur);
    Ok(())
}

pub fn pathbbox(ip: &mut Interp) -> R {
    ip.charge(ip.gs.path.len() as u64 / 4 + 1)?;
    let b = bbox(&ip.gs.path).ok_or(PsErr::Ps("nocurrentpoint"))?;
    let inv = ip.gs.ctm.inv().ok_or(PsErr::Ps("undefinedresult"))?;
    let corners = [inv.apply(b[0], b[1]), inv.apply(b[2], b[1]), inv.apply(b[0], b[3]), inv.apply(b[2], b[3])];
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (x, y) in corners {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    for v in [x0, y0, x1, y1] {
        ip.push(Obj::Real(v))?;
    }
    Ok(())
}

pub fn pathforall(ip: &mut Interp) -> R {
    ip.charge_path(ip.gs.path.len())?;
    let close = ip.pop()?;
    let curve = ip.pop()?;
    let line = ip.pop()?;
    let mv = ip.pop()?;
    let inv = ip.gs.ctm.inv().ok_or(PsErr::Ps("undefinedresult"))?;
    for s in ip.gs.path.clone() {
        let (proc, pts) = match s {
            Seg::Move(x, y) => (&mv, vec![inv.apply(x, y)]),
            Seg::Line(x, y) => (&line, vec![inv.apply(x, y)]),
            Seg::Curve(a, b, c, d, e, f) => (&curve, vec![inv.apply(a, b), inv.apply(c, d), inv.apply(e, f)]),
            Seg::Close => (&close, vec![]),
        };
        for (x, y) in pts {
            ip.push(Obj::Real(x))?;
            ip.push(Obj::Real(y))?;
        }
        ip.call(proc.clone())?;
    }
    Ok(())
}

pub fn rect_path(ip: &mut Interp) -> R<Vec<Seg>> {
    let mut rects: Vec<[f64; 4]> = Vec::new();
    match ip.pop()? {
        Obj::Array(a, _) => {
            ip.charge_path(a.len)?;
            let v: Vec<f64> = a.items().iter().filter_map(Obj::num).collect();
            for r in v.as_chunks::<4>().0 {
                rects.push(*r);
            }
        }
        Obj::Str(_, _) => return err("typecheck"),
        h => {
            let h = h.num().ok_or(PsErr::Ps("typecheck"))?;
            let w = ip.pop_num()?;
            let y = ip.pop_num()?;
            let x = ip.pop_num()?;
            rects.push([x, y, w, h]);
        }
    }
    let m = ip.gs.ctm;
    let mut path = Vec::with_capacity(rects.len() * 5);
    for [x, y, w, h] in rects {
        let p = |a: f64, b: f64| m.apply(a, b);
        let (a, b, c, d) = (p(x, y), p(x + w, y), p(x + w, y + h), p(x, y + h));
        path.extend([Seg::Move(a.0, a.1), Seg::Line(b.0, b.1), Seg::Line(c.0, c.1), Seg::Line(d.0, d.1), Seg::Close]);
    }
    Ok(path)
}

// ----- painting -----

pub fn fill_path(ip: &mut Interp, path: &[Seg], eo: bool) -> R {
    ip.charge(path.len() as u64 / 4 + 1)?;
    if ip.gs.nulldev {
        return Ok(());
    }
    if matches!(ip.gs.space, Space::Pattern) && crate::ops::shading::fill_pattern(ip, path, eo)? {
        return Ok(());
    }
    let color = current_color(ip)?;
    ip.out.fill(&ip.gs.clip, path, eo, color).map_err(PsErr::Fatal)
}

pub fn fill(ip: &mut Interp) -> R {
    let path = std::mem::take(&mut ip.gs.path);
    fill_path(ip, &path, false)?;
    newpath(ip)
}

pub fn eofill(ip: &mut Interp) -> R {
    let path = std::mem::take(&mut ip.gs.path);
    fill_path(ip, &path, true)?;
    newpath(ip)
}

pub fn stroke_path(ip: &mut Interp, path: &[Seg]) -> R {
    ip.charge_path(path.len())?;
    if ip.gs.nulldev {
        return Ok(());
    }
    let color = current_color(ip)?;
    let g = &ip.gs;
    let p = StrokeParams { width: g.line_width, cap: g.cap, join: g.join, miter: g.miter, dash: &g.dash };
    ip.out.stroke(&g.clip, path, &g.ctm, &p, color).map_err(PsErr::Fatal)
}

pub fn stroke(ip: &mut Interp) -> R {
    let path = std::mem::take(&mut ip.gs.path);
    stroke_path(ip, &path)?;
    newpath(ip)
}

pub fn rectfill(ip: &mut Interp) -> R {
    let path = rect_path(ip)?;
    fill_path(ip, &path, false)
}

pub fn rectstroke(ip: &mut Interp) -> R {
    // An optional matrix operand changes the pen: applied to the CTM for this stroke.
    let m = match ip.top(0)? {
        Obj::Array(a, _) if a.len == 6 && ip.o.len() >= 2 && !matches!(ip.top(1), Ok(Obj::Array(..)) | Err(_)) => Some(ip.pop_matrix()?),
        _ => None,
    };
    let path = rect_path(ip)?;
    match m {
        Some(m) => {
            let saved = ip.gs.ctm;
            ip.gs.ctm = m.then(&saved);
            let r = stroke_path(ip, &path);
            ip.gs.ctm = saved;
            r
        }
        None => stroke_path(ip, &path),
    }
}

fn clip_with(ip: &mut Interp, path: Vec<Seg>, eo: bool) -> R {
    let depth = ip.gs.clip.as_ref().map_or(0, |c| c.depth + 1);
    if depth > 2000 {
        return err("limitcheck");
    }
    ip.gs.clip = Some(Rc::new(Clip { path, eo, parent: ip.gs.clip.clone(), depth }));
    Ok(())
}

pub fn clip(ip: &mut Interp) -> R {
    ip.charge_path(ip.gs.path.len())?;
    let path = ip.gs.path.clone();
    clip_with(ip, path, false)
}

pub fn eoclip(ip: &mut Interp) -> R {
    ip.charge_path(ip.gs.path.len())?;
    let path = ip.gs.path.clone();
    clip_with(ip, path, true)
}

pub fn rectclip(ip: &mut Interp) -> R {
    let path = rect_path(ip)?;
    clip_with(ip, path, false)?;
    newpath(ip)
}

pub fn clipsave(ip: &mut Interp) -> R {
    if ip.gs.clip_saves.len() >= 1000 {
        return err("limitcheck");
    }
    let c = ip.gs.clip.clone();
    ip.gs.clip_saves.push(c);
    Ok(())
}

pub fn cliprestore(ip: &mut Interp) -> R {
    if let Some(c) = ip.gs.clip_saves.pop() {
        ip.gs.clip = c;
    }
    Ok(())
}

pub fn initclip(ip: &mut Interp) -> R {
    ip.gs.clip = None;
    Ok(())
}

pub fn clippath(ip: &mut Interp) -> R {
    let n = ip.gs.clip.as_ref().map_or(0, |c| c.path.len());
    ip.charge_path(n)?;
    let path = match &ip.gs.clip {
        Some(c) => c.path.clone(),
        None => {
            // The whole page: a generous square.
            let s = 1.0e5;
            vec![Seg::Move(-s, -s), Seg::Line(s, -s), Seg::Line(s, s), Seg::Line(-s, s), Seg::Close]
        }
    };
    ip.gs.cur = path.iter().rev().find_map(|s| match s {
        Seg::Move(x, y) | Seg::Line(x, y) => Some((*x, *y)),
        _ => None,
    });
    ip.gs.path = path;
    Ok(())
}

// ----- colour -----

/// A colour space from a name or array operand.
pub fn parse_space(ip: &mut Interp, o: &Obj, depth: usize) -> R<Space> {
    if depth > 8 {
        return err("limitcheck");
    }
    let (family, params) = match o {
        Obj::Name(n, _) => (n.to_vec(), Vec::new()),
        Obj::Array(a, _) => {
            let items = a.items();
            match items.first() {
                Some(Obj::Name(n, _)) => (n.to_vec(), items.get(1..).map(<[Obj]>::to_vec).unwrap_or_default()),
                _ => return err("typecheck"),
            }
        }
        _ => return err("typecheck"),
    };
    Ok(match family.as_slice() {
        b"DeviceGray" | b"CalGray" | b"G" | b"CIEBasedA" => Space::Gray,
        b"DeviceRGB" | b"CalRGB" | b"RGB" | b"CIEBasedABC" | b"CIEBasedDEF" => Space::Rgb,
        b"DeviceCMYK" | b"CMYK" | b"CIEBasedDEFG" => Space::Cmyk,
        b"Lab" => Space::Lab,
        b"Pattern" => Space::Pattern,
        b"ICCBased" => {
            let n = match params.first() {
                Some(Obj::Dict(d)) => d.get_name(b"N").and_then(|o| o.num()).unwrap_or(3.0) as usize,
                _ => 3,
            };
            match n {
                1 => Space::Gray,
                4 => Space::Cmyk,
                _ => Space::Rgb,
            }
        }
        b"Separation" | b"DeviceN" => {
            let names = match params.first() {
                Some(Obj::Array(a, _)) => a.items().iter().map(crate::ops::basic::text_of).collect(),
                Some(o) => vec![crate::ops::basic::text_of(o)],
                None => return err("rangecheck"),
            };
            let alt = parse_space(ip, params.get(1).ok_or(PsErr::Ps("rangecheck"))?, depth + 1)?;
            let tint = params.get(2).cloned().ok_or(PsErr::Ps("rangecheck"))?;
            Space::Tint { names, alt: Box::new(alt), tint }
        }
        b"Indexed" | b"I" => {
            let base = parse_space(ip, params.first().ok_or(PsErr::Ps("rangecheck"))?, depth + 1)?;
            let hival = params.get(1).and_then(Obj::num).unwrap_or(0.0).clamp(0.0, 4095.0) as usize;
            let n = base.comps();
            let lookup = match params.get(2) {
                Some(Obj::Str(s, _)) => s.bytes(),
                Some(p @ Obj::Array(_, true)) => {
                    let mut out = Vec::with_capacity((hival + 1) * n);
                    for i in 0..=hival {
                        ip.push(Obj::Int(i as i64))?;
                        ip.call(p.clone())?;
                        let mut comps = vec![0.0; n];
                        for c in comps.iter_mut().rev() {
                            *c = ip.pop_num()?;
                        }
                        out.extend(comps.iter().map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8));
                    }
                    out
                }
                _ => return err("typecheck"),
            };
            Space::Indexed { base: Box::new(base), hival, lookup }
        }
        _ => return err("undefined"),
    })
}

/// Evaluate a tint transform (a procedure or a Type 2 function dictionary) on `comps`.
fn eval_tint(ip: &mut Interp, tint: &Obj, comps: &[f64], out_n: usize) -> R<Vec<f64>> {
    match tint {
        Obj::Dict(d) => {
            // Type 2 exponential: C0 + x^N (C1 − C0).
            let get = |k: &[u8], dflt: f64| -> Vec<f64> {
                match d.get_name(k) {
                    Some(Obj::Array(a, _)) => a.items().iter().filter_map(Obj::num).collect(),
                    _ => vec![dflt; out_n],
                }
            };
            let c0 = get(b"C0", 0.0);
            let c1 = get(b"C1", 1.0);
            let n = d.get_name(b"N").and_then(|o| o.num()).unwrap_or(1.0);
            let x = comps.first().copied().unwrap_or(0.0).clamp(0.0, 1.0).powf(n);
            Ok((0..out_n)
                .map(|i| c0.get(i).copied().unwrap_or(0.0) + x * (c1.get(i).copied().unwrap_or(1.0) - c0.get(i).copied().unwrap_or(0.0)))
                .collect())
        }
        _ => {
            for c in comps {
                ip.push(Obj::Real(*c))?;
            }
            ip.call(tint.clone())?;
            let mut v = vec![0.0; out_n];
            for slot in v.iter_mut().rev() {
                *slot = ip.pop_num()?;
            }
            Ok(v)
        }
    }
}

pub fn lab_to_rgb(l: f64, a: f64, b: f64) -> (f64, f64, f64) {
    let fy = (l + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;
    let g = |t: f64| if t > 6.0 / 29.0 { t * t * t } else { 3.0 * (6.0f64 / 29.0).powi(2) * (t - 4.0 / 29.0) };
    let (x, y, z) = (0.9505 * g(fx), g(fy), 1.089 * g(fz));
    let lin = [3.2406 * x - 1.5372 * y - 0.4986 * z, -0.9689 * x + 1.8758 * y + 0.0415 * z, 0.0557 * x - 0.2040 * y + 1.0570 * z];
    let enc = |c: f64| {
        let c = c.clamp(0.0, 1.0);
        if c <= 0.0031308 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
    };
    (enc(lin[0]), enc(lin[1]), enc(lin[2]))
}

/// A colour in `space` as a device colour.
pub fn to_device(ip: &mut Interp, space: &Space, comps: &[f64], depth: usize) -> R<DevColor> {
    if depth > 8 {
        return err("limitcheck");
    }
    let c = |i: usize| comps.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);
    Ok(match space {
        Space::Gray => DevColor::Gray(c(0)),
        Space::Rgb => DevColor::Rgb(c(0), c(1), c(2)),
        Space::Cmyk => DevColor::Cmyk(c(0), c(1), c(2), c(3)),
        Space::Lab => {
            let (r, g, b) =
                lab_to_rgb(comps.first().copied().unwrap_or(0.0), comps.get(1).copied().unwrap_or(0.0), comps.get(2).copied().unwrap_or(0.0));
            DevColor::Rgb(r, g, b)
        }
        Space::Tint { names, alt, tint } => {
            // The colourant "None" paints nothing; "All" is registration black.
            if names.len() == 1 && names.first().is_some_and(|n| n == b"All") {
                return Ok(DevColor::Cmyk(c(0), c(0), c(0), c(0)));
            }
            let v = eval_tint(ip, &tint.clone(), comps, alt.comps())?;
            to_device(ip, &alt.clone(), &v, depth + 1)?
        }
        Space::Indexed { base, hival, lookup } => {
            let i = (comps.first().copied().unwrap_or(0.0).round().max(0.0) as usize).min(*hival);
            let n = base.comps();
            let v: Vec<f64> = (0..n).map(|k| f64::from(lookup.get(i * n + k).copied().unwrap_or(0)) / 255.0).collect();
            to_device(ip, &base.clone(), &v, depth + 1)?
        }
        // Patterns aren't painted yet: a mid grey stands in.
        Space::Pattern => DevColor::Gray(0.5),
    })
}

pub fn current_color(ip: &mut Interp) -> R<DevColor> {
    let space = ip.gs.space.clone();
    let comps = ip.gs.color.clone();
    to_device(ip, &space, &comps, 0)
}

fn set_space(ip: &mut Interp, space: Space) {
    if ip.gs.color_locked {
        return;
    }
    ip.gs.color = space.initial();
    ip.gs.space = space;
    ip.gs.pattern = None;
}

/// Set the colour values (not inside an uncoloured pattern cell).
fn set_color(ip: &mut Interp, v: Vec<f64>) {
    if !ip.gs.color_locked {
        ip.gs.color = v;
    }
}

pub fn setgray(ip: &mut Interp) -> R {
    let g = ip.pop_num()?;
    set_space(ip, Space::Gray);
    set_color(ip, vec![g]);
    Ok(())
}

pub fn setrgbcolor(ip: &mut Interp) -> R {
    let b = ip.pop_num()?;
    let g = ip.pop_num()?;
    let r = ip.pop_num()?;
    set_space(ip, Space::Rgb);
    set_color(ip, vec![r, g, b]);
    Ok(())
}

pub fn setcmykcolor(ip: &mut Interp) -> R {
    let k = ip.pop_num()?;
    let y = ip.pop_num()?;
    let m = ip.pop_num()?;
    let c = ip.pop_num()?;
    set_space(ip, Space::Cmyk);
    set_color(ip, vec![c, m, y, k]);
    Ok(())
}

pub fn sethsbcolor(ip: &mut Interp) -> R {
    let v = ip.pop_num()?.clamp(0.0, 1.0);
    let s = ip.pop_num()?.clamp(0.0, 1.0);
    let h = ip.pop_num()?.rem_euclid(1.0) * 6.0;
    let i = h.floor();
    let f = h - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match i as i64 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    set_space(ip, Space::Rgb);
    set_color(ip, vec![r, g, b]);
    Ok(())
}

pub fn currentgray(ip: &mut Interp) -> R {
    let g = match current_color(ip)? {
        DevColor::Gray(g) => g,
        DevColor::Rgb(r, g, b) => 0.3 * r + 0.59 * g + 0.11 * b,
        DevColor::Cmyk(c, m, y, k) => 1.0 - (0.3 * c + 0.59 * m + 0.11 * y + k).min(1.0),
    };
    ip.push(Obj::Real(g))
}

fn rgb_of(d: DevColor) -> (f64, f64, f64) {
    match d {
        DevColor::Gray(g) => (g, g, g),
        DevColor::Rgb(r, g, b) => (r, g, b),
        DevColor::Cmyk(c, m, y, k) => (1.0 - (c + k).min(1.0), 1.0 - (m + k).min(1.0), 1.0 - (y + k).min(1.0)),
    }
}

pub fn currentrgbcolor(ip: &mut Interp) -> R {
    let (r, g, b) = rgb_of(current_color(ip)?);
    for v in [r, g, b] {
        ip.push(Obj::Real(v))?;
    }
    Ok(())
}

pub fn currenthsbcolor(ip: &mut Interp) -> R {
    let (r, g, b) = rgb_of(current_color(ip)?);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    for v in [h, s, max] {
        ip.push(Obj::Real(v))?;
    }
    Ok(())
}

pub fn currentcmykcolor(ip: &mut Interp) -> R {
    let (c, m, y, k) = match current_color(ip)? {
        DevColor::Cmyk(c, m, y, k) => (c, m, y, k),
        DevColor::Gray(g) => (0.0, 0.0, 0.0, 1.0 - g),
        DevColor::Rgb(r, g, b) => {
            let (c, m, y) = (1.0 - r, 1.0 - g, 1.0 - b);
            let k = c.min(m).min(y);
            (c - k, m - k, y - k, k)
        }
    };
    for v in [c, m, y, k] {
        ip.push(Obj::Real(v))?;
    }
    Ok(())
}

pub fn setcolorspace(ip: &mut Interp) -> R {
    let o = ip.pop()?;
    let space = parse_space(ip, &o, 0)?;
    let base = match (&space, &o) {
        (Space::Pattern, Obj::Array(a, _)) => match a.get(1) {
            Some(b) => Some(Box::new(parse_space(ip, &b, 1)?)),
            None => None,
        },
        _ => None,
    };
    set_space(ip, space);
    if !ip.gs.color_locked {
        ip.gs.pattern_base = base;
    }
    Ok(())
}

pub fn currentcolorspace(ip: &mut Interp) -> R {
    let n: &[u8] = match ip.gs.space {
        Space::Gray => b"DeviceGray",
        Space::Rgb => b"DeviceRGB",
        Space::Cmyk => b"DeviceCMYK",
        Space::Pattern => b"Pattern",
        Space::Lab => b"Lab",
        Space::Indexed { .. } => b"Indexed",
        Space::Tint { .. } => b"Separation",
    };
    ip.push(Obj::Array(PArr::new(vec![Obj::lit(n)]), false))
}

pub fn setcolor(ip: &mut Interp) -> R {
    if let Space::Pattern = ip.gs.space {
        // An uncoloured pattern comes after its colour: `c1 … cn pattern setcolor`.
        if let Obj::Dict(d) = ip.pop()?
            && !ip.gs.color_locked
        {
            let uncolored = d.get_name(b"PaintType").and_then(|o| o.num()) == Some(2.0);
            let mut under = Vec::new();
            while uncolored && ip.top(0).is_ok_and(|o| o.num().is_some()) && under.len() < 4 {
                under.push(ip.pop_num()?);
            }
            under.reverse();
            ip.gs.color = under;
            ip.gs.pattern = Some(d);
        }
        return Ok(());
    }
    let n = ip.gs.space.comps();
    let mut v = vec![0.0; n];
    for slot in v.iter_mut().rev() {
        *slot = ip.pop_num()?;
    }
    set_color(ip, v);
    Ok(())
}

pub fn currentcolor(ip: &mut Interp) -> R {
    for v in ip.gs.color.clone() {
        ip.push(Obj::Real(v))?;
    }
    Ok(())
}

pub fn setpattern(ip: &mut Interp) -> R {
    let d: PDict = ip.pop_dict()?;
    if ip.gs.color_locked {
        return Ok(());
    }
    // Under an uncoloured pattern, the current colour stays its colour.
    let under = ip.gs.color.clone();
    set_space(ip, Space::Pattern);
    if d.get_name(b"PaintType").and_then(|o| o.num()) == Some(2.0) {
        ip.gs.color = under;
    }
    ip.gs.pattern = Some(d);
    Ok(())
}

/// A pattern instance: a copy of the prototype with its pattern space (matrix · CTM).
pub fn makepattern(ip: &mut Interp) -> R {
    let m = ip.pop_matrix()?;
    let d = ip.pop_dict()?;
    let inst = PDict::new(d.len() + 2);
    for (k, v) in d.entries() {
        inst.put(k, v);
    }
    inst.put_name(b"DesignCraftMatrix", matrix_obj(&m.then(&ip.gs.ctm)));
    ip.push(Obj::Dict(inst))
}
