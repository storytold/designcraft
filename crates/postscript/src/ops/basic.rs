//! The language: stack, arithmetic, relations, control, types, dictionaries, strings, arrays.

use crate::interp::{Frame, Interp, PsErr, R, err};
use crate::obj::{Key, Obj, PArr, PDict, PStr, name};

// ----- stack -----

pub fn pop(ip: &mut Interp) -> R {
    ip.pop().map(|_| ())
}

pub fn exch(ip: &mut Interp) -> R {
    ip.need(2)?;
    let n = ip.o.len();
    ip.o.swap(n - 1, n - 2);
    Ok(())
}

pub fn dup(ip: &mut Interp) -> R {
    let o = ip.top(0)?.clone();
    ip.push(o)
}

pub fn copy(ip: &mut Interp) -> R {
    match ip.pop()? {
        Obj::Int(n) => {
            let n = usize::try_from(n).map_err(|_| PsErr::Ps("rangecheck"))?;
            ip.need(n)?;
            let start = ip.o.len() - n;
            ip.charge(n as u64)?;
            ip.alloc(n.saturating_mul(OBJ_BYTES))?;
            let items: Vec<Obj> = ip.o.get(start..).map(<[Obj]>::to_vec).unwrap_or_default();
            for o in items {
                ip.push(o)?;
            }
            Ok(())
        }
        Obj::Array(dst, x) => {
            let src = ip.pop_arr()?;
            if src.len > dst.len {
                return err("rangecheck");
            }
            ip.charge(src.len as u64)?;
            ip.alloc(src.len.saturating_mul(OBJ_BYTES))?;
            for (i, o) in src.items().into_iter().enumerate() {
                dst.set(i, o);
            }
            let sub = dst.interval(0, src.len).ok_or(PsErr::Ps("rangecheck"))?;
            ip.push(Obj::Array(sub, x))
        }
        Obj::Str(dst, x) => {
            let src = ip.pop_str()?;
            ip.charge_bytes(src.len)?;
            ip.alloc(src.len)?;
            let w = dst.put_bytes(0, &src.bytes()).ok_or(PsErr::Ps("rangecheck"))?;
            ip.push(Obj::Str(w, x))
        }
        Obj::Dict(dst) => {
            let src = ip.pop_dict()?;
            ip.charge(src.len() as u64)?;
            ip.alloc(src.len().saturating_mul(2 * OBJ_BYTES))?;
            for (k, v) in src.entries() {
                dst.put(k, v);
            }
            ip.push(Obj::Dict(dst))
        }
        _ => err("typecheck"),
    }
}

pub fn index(ip: &mut Interp) -> R {
    let n = ip.pop_usize()?;
    let o = ip.top(n).map_err(|_| PsErr::Ps("rangecheck"))?.clone();
    ip.push(o)
}

pub fn roll(ip: &mut Interp) -> R {
    let j = ip.pop_int()?;
    let n = ip.pop_usize()?;
    ip.need(n)?;
    if n == 0 {
        return Ok(());
    }
    let start = ip.o.len() - n;
    let k = usize::try_from(j.rem_euclid(n as i64)).unwrap_or(0);
    if let Some(s) = ip.o.get_mut(start..) {
        s.rotate_right(k);
    }
    Ok(())
}

pub fn clear(ip: &mut Interp) -> R {
    ip.o.clear();
    Ok(())
}

pub fn count(ip: &mut Interp) -> R {
    let n = ip.o.len() as i64;
    ip.push(Obj::Int(n))
}

pub fn mark(ip: &mut Interp) -> R {
    ip.push(Obj::Mark)
}

/// Where the topmost mark is; the search is work (a deep stack takes long).
pub fn mark_pos(ip: &mut Interp) -> R<usize> {
    let i = ip.o.iter().rposition(|o| matches!(o, Obj::Mark));
    ip.charge((ip.o.len() - i.map_or(0, |i| i + 1)) as u64)?;
    i.ok_or(PsErr::Ps("unmatchedmark"))
}

/// Memory one object takes, for charging copies of arrays and stacks.
pub const OBJ_BYTES: usize = 32;

/// A string used as a dictionary key is copied into a name: work and memory.
pub fn key_cost(ip: &mut Interp, k: &Obj) -> R {
    if let Obj::Str(s, _) = k {
        ip.charge_bytes(s.len)?;
        ip.alloc(s.len)?;
    }
    Ok(())
}

pub fn cleartomark(ip: &mut Interp) -> R {
    let i = mark_pos(ip)?;
    ip.o.truncate(i);
    Ok(())
}

pub fn counttomark(ip: &mut Interp) -> R {
    let i = mark_pos(ip)?;
    let n = (ip.o.len() - i - 1) as i64;
    ip.push(Obj::Int(n))
}

pub fn array_close(ip: &mut Interp) -> R {
    let i = mark_pos(ip)?;
    let items = ip.o.split_off(i + 1);
    ip.o.pop();
    ip.alloc(items.len().saturating_mul(OBJ_BYTES))?;
    ip.push(Obj::Array(PArr::new(items), false))
}

pub fn dict_close(ip: &mut Interp) -> R {
    let i = mark_pos(ip)?;
    let items = ip.o.split_off(i + 1);
    ip.o.pop();
    if !items.len().is_multiple_of(2) {
        return err("rangecheck");
    }
    ip.alloc(items.len().saturating_mul(OBJ_BYTES))?;
    for k in items.iter().step_by(2) {
        key_cost(ip, k)?;
    }
    let d = PDict::new(items.len() / 2);
    let mut it = items.into_iter();
    while let (Some(k), Some(v)) = (it.next(), it.next()) {
        d.put(k, v);
    }
    ip.push(Obj::Dict(d))
}

// ----- arithmetic -----

fn arith(ip: &mut Interp, fi: fn(i64, i64) -> Option<i64>, fr: fn(f64, f64) -> f64) -> R {
    let b = ip.pop()?;
    let a = ip.pop()?;
    match (&a, &b) {
        (Obj::Int(x), Obj::Int(y)) => match fi(*x, *y).filter(|v| i32::try_from(*v).is_ok()) {
            Some(v) => ip.push(Obj::Int(v)),
            None => ip.push(Obj::Real(fr(*x as f64, *y as f64))),
        },
        _ => {
            let (Some(x), Some(y)) = (a.num(), b.num()) else { return err("typecheck") };
            let v = fr(x, y);
            if !v.is_finite() {
                return err("undefinedresult");
            }
            ip.push(Obj::Real(v))
        }
    }
}

pub fn add(ip: &mut Interp) -> R {
    arith(ip, i64::checked_add, |a, b| a + b)
}

pub fn sub(ip: &mut Interp) -> R {
    arith(ip, i64::checked_sub, |a, b| a - b)
}

pub fn mul(ip: &mut Interp) -> R {
    arith(ip, i64::checked_mul, |a, b| a * b)
}

pub fn div(ip: &mut Interp) -> R {
    let b = ip.pop_num()?;
    let a = ip.pop_num()?;
    if b == 0.0 {
        return err("undefinedresult");
    }
    ip.push(Obj::Real(a / b))
}

pub fn idiv(ip: &mut Interp) -> R {
    let b = ip.pop_int()?;
    let a = ip.pop_int()?;
    let v = a.checked_div(b).ok_or(PsErr::Ps("undefinedresult"))?;
    ip.push(Obj::Int(v))
}

pub fn mod_(ip: &mut Interp) -> R {
    let b = ip.pop_int()?;
    let a = ip.pop_int()?;
    let v = a.checked_rem(b).ok_or(PsErr::Ps("undefinedresult"))?;
    ip.push(Obj::Int(v))
}

fn unary(ip: &mut Interp, fi: fn(i64) -> Option<i64>, fr: fn(f64) -> f64) -> R {
    match ip.pop()? {
        Obj::Int(i) => match fi(i) {
            Some(v) => ip.push(Obj::Int(v)),
            None => ip.push(Obj::Real(fr(i as f64))),
        },
        Obj::Real(r) => ip.push(Obj::Real(fr(r))),
        _ => err("typecheck"),
    }
}

pub fn neg(ip: &mut Interp) -> R {
    unary(ip, i64::checked_neg, |r| -r)
}

pub fn abs(ip: &mut Interp) -> R {
    unary(ip, i64::checked_abs, f64::abs)
}

pub fn ceiling(ip: &mut Interp) -> R {
    unary(ip, Some, f64::ceil)
}

pub fn floor(ip: &mut Interp) -> R {
    unary(ip, Some, f64::floor)
}

pub fn round(ip: &mut Interp) -> R {
    // PostScript rounds halves up (toward +∞).
    unary(ip, Some, |r| (r + 0.5).floor())
}

pub fn truncate(ip: &mut Interp) -> R {
    unary(ip, Some, f64::trunc)
}

fn real_fn(ip: &mut Interp, f: fn(f64) -> f64) -> R {
    let a = ip.pop_num()?;
    let v = f(a);
    if !v.is_finite() {
        return err("undefinedresult");
    }
    ip.push(Obj::Real(v))
}

pub fn sqrt(ip: &mut Interp) -> R {
    let a = ip.pop_num()?;
    if a < 0.0 {
        return err("rangecheck");
    }
    ip.push(Obj::Real(a.sqrt()))
}

pub fn sin(ip: &mut Interp) -> R {
    real_fn(ip, |d| d.to_radians().sin())
}

pub fn cos(ip: &mut Interp) -> R {
    real_fn(ip, |d| d.to_radians().cos())
}

pub fn atan(ip: &mut Interp) -> R {
    let den = ip.pop_num()?;
    let num = ip.pop_num()?;
    if num == 0.0 && den == 0.0 {
        return err("undefinedresult");
    }
    let mut a = num.atan2(den).to_degrees();
    if a < 0.0 {
        a += 360.0;
    }
    ip.push(Obj::Real(a))
}

pub fn exp(ip: &mut Interp) -> R {
    let e = ip.pop_num()?;
    let b = ip.pop_num()?;
    let v = b.powf(e);
    if !v.is_finite() {
        return err("undefinedresult");
    }
    ip.push(Obj::Real(v))
}

pub fn ln(ip: &mut Interp) -> R {
    let a = ip.pop_num()?;
    if a <= 0.0 {
        return err("rangecheck");
    }
    ip.push(Obj::Real(a.ln()))
}

pub fn log(ip: &mut Interp) -> R {
    let a = ip.pop_num()?;
    if a <= 0.0 {
        return err("rangecheck");
    }
    ip.push(Obj::Real(a.log10()))
}

pub fn rand(ip: &mut Interp) -> R {
    // A fixed, repeatable sequence (no entropy in conversions).
    ip.steps = ip.steps.wrapping_add(1);
    let v = (ip.steps.wrapping_mul(1_103_515_245).wrapping_add(12345) >> 1) & 0x7FFF_FFFF;
    ip.push(Obj::Int(v as i64))
}

pub fn srand(ip: &mut Interp) -> R {
    ip.pop_int().map(|_| ())
}

pub fn rrand(ip: &mut Interp) -> R {
    ip.push(Obj::Int(0))
}

// ----- relations, booleans, bits -----

/// Comparing strings compares (and copies) their bytes.
fn compare_cost(ip: &mut Interp, a: &Obj, b: &Obj) -> R {
    let len = |o: &Obj| match o {
        Obj::Str(s, _) => s.len,
        Obj::Name(n, _) => n.len(),
        _ => 0,
    };
    if matches!(a, Obj::Str(..)) || matches!(b, Obj::Str(..)) {
        let n = len(a).saturating_add(len(b));
        ip.charge_bytes(n)?;
        ip.alloc(n)?;
    }
    Ok(())
}

pub fn eq(ip: &mut Interp) -> R {
    let b = ip.pop()?;
    let a = ip.pop()?;
    compare_cost(ip, &a, &b)?;
    ip.push(Obj::Bool(a.ps_eq(&b)))
}

pub fn ne(ip: &mut Interp) -> R {
    let b = ip.pop()?;
    let a = ip.pop()?;
    compare_cost(ip, &a, &b)?;
    ip.push(Obj::Bool(!a.ps_eq(&b)))
}

fn compare(ip: &mut Interp, f: fn(std::cmp::Ordering) -> bool) -> R {
    let b = ip.pop()?;
    let a = ip.pop()?;
    compare_cost(ip, &a, &b)?;
    let ord = match (&a, &b) {
        (Obj::Str(x, _), Obj::Str(y, _)) => x.bytes().cmp(&y.bytes()),
        _ => {
            let (Some(x), Some(y)) = (a.num(), b.num()) else { return err("typecheck") };
            x.partial_cmp(&y).ok_or(PsErr::Ps("undefinedresult"))?
        }
    };
    ip.push(Obj::Bool(f(ord)))
}

pub fn gt(ip: &mut Interp) -> R {
    compare(ip, |o| o.is_gt())
}

pub fn ge(ip: &mut Interp) -> R {
    compare(ip, |o| o.is_ge())
}

pub fn lt(ip: &mut Interp) -> R {
    compare(ip, |o| o.is_lt())
}

pub fn le(ip: &mut Interp) -> R {
    compare(ip, |o| o.is_le())
}

fn logic(ip: &mut Interp, fb: fn(bool, bool) -> bool, fi: fn(i64, i64) -> i64) -> R {
    let b = ip.pop()?;
    let a = ip.pop()?;
    match (a, b) {
        (Obj::Bool(x), Obj::Bool(y)) => ip.push(Obj::Bool(fb(x, y))),
        (Obj::Int(x), Obj::Int(y)) => ip.push(Obj::Int(fi(x, y))),
        _ => err("typecheck"),
    }
}

pub fn and(ip: &mut Interp) -> R {
    logic(ip, |a, b| a & b, |a, b| a & b)
}

pub fn or(ip: &mut Interp) -> R {
    logic(ip, |a, b| a | b, |a, b| a | b)
}

pub fn xor(ip: &mut Interp) -> R {
    logic(ip, |a, b| a ^ b, |a, b| a ^ b)
}

pub fn not(ip: &mut Interp) -> R {
    match ip.pop()? {
        Obj::Bool(b) => ip.push(Obj::Bool(!b)),
        Obj::Int(i) => ip.push(Obj::Int(!i)),
        _ => err("typecheck"),
    }
}

pub fn bitshift(ip: &mut Interp) -> R {
    let s = ip.pop_int()?;
    let v = ip.pop_int()? as i32 as u32;
    let r = if s >= 0 { v.checked_shl(s.min(32) as u32).unwrap_or(0) } else { v.checked_shr((-s).min(32) as u32).unwrap_or(0) };
    ip.push(Obj::Int(i64::from(r as i32)))
}

pub fn true_(ip: &mut Interp) -> R {
    ip.push(Obj::Bool(true))
}

pub fn false_(ip: &mut Interp) -> R {
    ip.push(Obj::Bool(false))
}

pub fn null(ip: &mut Interp) -> R {
    ip.push(Obj::Null)
}

// ----- control -----

pub fn exec(ip: &mut Interp) -> R {
    let o = ip.pop()?;
    ip.exec_value(o)
}

pub fn if_(ip: &mut Interp) -> R {
    let p = ip.pop()?;
    if ip.pop_bool()? { ip.exec_value(p) } else { Ok(()) }
}

pub fn ifelse(ip: &mut Interp) -> R {
    let pf = ip.pop()?;
    let pt = ip.pop()?;
    if ip.pop_bool()? { ip.exec_value(pt) } else { ip.exec_value(pf) }
}

pub fn for_(ip: &mut Interp) -> R {
    let proc = ip.pop()?;
    let limit = ip.pop()?;
    let inc = ip.pop()?;
    let init = ip.pop()?;
    let int = matches!((&init, &inc), (Obj::Int(_), Obj::Int(_)));
    let (Some(cur), Some(inc), Some(limit)) = (init.num(), inc.num(), limit.num()) else { return err("typecheck") };
    if inc == 0.0 {
        // Runs forever in PostScript; the step limit would end it. Treat as empty.
        return Ok(());
    }
    ip.push_frame(Frame::For { cur, inc, limit, int, proc })
}

pub fn repeat(ip: &mut Interp) -> R {
    let proc = ip.pop()?;
    let n = ip.pop_int()?;
    if n < 0 {
        return err("rangecheck");
    }
    ip.push_frame(Frame::Repeat { n, proc })
}

pub fn loop_(ip: &mut Interp) -> R {
    let proc = ip.pop()?;
    ip.push_frame(Frame::Loop { proc })
}

pub fn exit(ip: &mut Interp) -> R {
    let depth = ip.e.len() as u64;
    ip.charge(depth / 16)?;
    let i = ip.e.iter().rposition(|f| f.is_loop() || matches!(f, Frame::Stopped | Frame::Barrier)).ok_or(PsErr::Ps("invalidexit"))?;
    if !ip.e.get(i).is_some_and(Frame::is_loop) {
        return err("invalidexit");
    }
    ip.e.truncate(i);
    Ok(())
}

pub fn stop(_ip: &mut Interp) -> R {
    Err(PsErr::Stop)
}

pub fn stopped(ip: &mut Interp) -> R {
    let p = ip.pop()?;
    ip.push_frame(Frame::Stopped)?;
    ip.exec_value(p)
}

pub fn quit(_ip: &mut Interp) -> R {
    Err(PsErr::Quit)
}

pub fn forall(ip: &mut Interp) -> R {
    let proc = ip.pop()?;
    match ip.pop()? {
        Obj::Array(arr, _) => ip.push_frame(Frame::ForallArr { arr, i: 0, proc }),
        Obj::Str(s, _) => ip.push_frame(Frame::ForallStr { s, i: 0, proc }),
        Obj::Dict(d) => {
            // The entries are copied first (the procedure may change the dictionary).
            ip.charge(d.len() as u64)?;
            ip.alloc(d.len().saturating_mul(2 * OBJ_BYTES))?;
            ip.push_frame(Frame::ForallDict { items: d.entries(), i: 0, proc })
        }
        _ => err("typecheck"),
    }
}

pub fn countexecstack(ip: &mut Interp) -> R {
    let n = ip.e.len() as i64;
    ip.push(Obj::Int(n))
}

pub fn execstack(ip: &mut Interp) -> R {
    let a = ip.pop_arr()?;
    let n = ip.e.len().min(a.len);
    let sub = a.interval(0, n).ok_or(PsErr::Ps("rangecheck"))?;
    ip.charge(n as u64)?;
    for i in 0..n {
        sub.set(i, Obj::Null);
    }
    ip.push(Obj::Array(sub, false))
}

// ----- types and conversion -----

pub fn type_(ip: &mut Interp) -> R {
    let o = ip.pop()?;
    ip.push(Obj::Name(name(o.type_name()), true))
}

pub fn cvlit(ip: &mut Interp) -> R {
    let o = match ip.pop()? {
        Obj::Name(n, _) => Obj::Name(n, false),
        Obj::Array(a, _) => Obj::Array(a, false),
        Obj::Str(s, _) => Obj::Str(s, false),
        o => o,
    };
    ip.push(o)
}

pub fn cvx(ip: &mut Interp) -> R {
    let o = match ip.pop()? {
        Obj::Name(n, _) => Obj::Name(n, true),
        Obj::Array(a, _) => Obj::Array(a, true),
        Obj::Str(s, _) => Obj::Str(s, true),
        o => o,
    };
    ip.push(o)
}

pub fn xcheck(ip: &mut Interp) -> R {
    let o = ip.pop()?;
    ip.push(Obj::Bool(o.is_exec()))
}

/// Access attributes aren't enforced: everything stays readable and writable.
pub fn noop_keep(ip: &mut Interp) -> R {
    ip.top(0).map(|_| ())
}

pub fn check_true(ip: &mut Interp) -> R {
    ip.pop()?;
    ip.push(Obj::Bool(true))
}

pub fn cvi(ip: &mut Interp) -> R {
    let v = match ip.pop()? {
        Obj::Int(i) => i,
        Obj::Real(r) if r.is_finite() && r.abs() < 2.2e9 => r.trunc() as i64,
        Obj::Str(s, _) if s.len > 4096 => return err("limitcheck"),
        Obj::Str(s, _) => match crate::scan::number(&s.bytes()) {
            Some(o) => o.num().filter(|v| v.abs() < 2.2e9).ok_or(PsErr::Ps("rangecheck"))?.trunc() as i64,
            None => return err("syntaxerror"),
        },
        Obj::Real(_) => return err("rangecheck"),
        _ => return err("typecheck"),
    };
    ip.push(Obj::Int(v))
}

pub fn cvr(ip: &mut Interp) -> R {
    let v = match ip.pop()? {
        o @ (Obj::Int(_) | Obj::Real(_)) => o.num().unwrap_or(0.0),
        Obj::Str(s, _) if s.len > 4096 => return err("limitcheck"),
        Obj::Str(s, _) => crate::scan::number(&s.bytes()).and_then(|o| o.num()).ok_or(PsErr::Ps("syntaxerror"))?,
        _ => return err("typecheck"),
    };
    ip.push(Obj::Real(v))
}

pub fn cvn(ip: &mut Interp) -> R {
    match ip.pop()? {
        Obj::Str(s, x) => {
            ip.charge_bytes(s.len)?;
            ip.alloc(s.len)?;
            ip.push(Obj::Name(name(&s.bytes()), x))
        }
        n @ Obj::Name(..) => ip.push(n),
        _ => err("typecheck"),
    }
}

/// The text `cvs`/`=` print for an object.
pub fn text_of(o: &Obj) -> Vec<u8> {
    match o {
        Obj::Int(i) => i.to_string().into_bytes(),
        Obj::Real(r) => {
            if r.fract() == 0.0 && r.abs() < 1e15 {
                format!("{r:.1}").into_bytes()
            } else {
                format!("{r}").into_bytes()
            }
        }
        Obj::Bool(b) => {
            if *b {
                b"true".to_vec()
            } else {
                b"false".to_vec()
            }
        }
        Obj::Name(n, _) => n.to_vec(),
        Obj::Str(s, _) => s.bytes(),
        Obj::Op(op) => crate::ops::OPS.get(*op as usize).map(|(n, _)| n.to_vec()).unwrap_or_default(),
        _ => b"--nostringval--".to_vec(),
    }
}

pub fn cvs(ip: &mut Interp) -> R {
    let dst = ip.pop_str()?;
    let o = ip.pop()?;
    ip.charge_bytes(dst.len)?;
    let t = text_of(&o);
    let w = dst.put_bytes(0, &t).ok_or(PsErr::Ps("rangecheck"))?;
    ip.push(Obj::Str(w, false))
}

pub fn cvrs(ip: &mut Interp) -> R {
    let dst = ip.pop_str()?;
    let radix = ip.pop_int()?;
    let n = ip.pop_num()?;
    if !(2..=36).contains(&radix) {
        return err("rangecheck");
    }
    let t = if radix == 10 {
        text_of(&Obj::Real(n))
    } else {
        let mut v = (n as i64 as i32) as u32;
        let mut digits = Vec::new();
        loop {
            let d = (v % radix as u32) as u8;
            digits.push(if d < 10 { b'0' + d } else { b'A' + d - 10 });
            v /= radix as u32;
            if v == 0 {
                break;
            }
        }
        digits.reverse();
        digits
    };
    let w = dst.put_bytes(0, &t).ok_or(PsErr::Ps("rangecheck"))?;
    ip.push(Obj::Str(w, false))
}

// ----- dictionaries -----

pub fn dict(ip: &mut Interp) -> R {
    let n = ip.pop_usize()?;
    ip.alloc(64)?;
    ip.push(Obj::Dict(PDict::new(n.min(100_000))))
}

pub fn maxlength(ip: &mut Interp) -> R {
    let d = ip.pop_dict()?;
    let n = d.0.borrow().cap.max(d.len()) as i64;
    ip.push(Obj::Int(n))
}

pub fn begin(ip: &mut Interp) -> R {
    let d = ip.pop_dict()?;
    ip.begin(d)
}

pub fn end(ip: &mut Interp) -> R {
    // systemdict, globaldict and userdict stay.
    if ip.d.len() <= 3 {
        return err("dictstackunderflow");
    }
    ip.d.pop();
    Ok(())
}

pub fn def(ip: &mut Interp) -> R {
    let v = ip.pop()?;
    let k = ip.pop()?;
    key_cost(ip, &k)?;
    ip.alloc(2 * OBJ_BYTES)?;
    ip.current_dict()?.put(k, v);
    Ok(())
}

pub fn load(ip: &mut Interp) -> R {
    let k = ip.pop()?;
    key_cost(ip, &k)?;
    let Some(v) = ip.lookup(&k.key()) else {
        ip.dollar_error.put_name(b"offending", k);
        return err("undefined");
    };
    ip.push(v)
}

pub fn store(ip: &mut Interp) -> R {
    let v = ip.pop()?;
    let k = ip.pop()?;
    key_cost(ip, &k)?;
    let key = k.key();
    let d = ip.d.iter().rev().find(|d| d.known(&key)).cloned().map_or_else(|| ip.current_dict(), Ok)?;
    d.put(k, v);
    Ok(())
}

pub fn get(ip: &mut Interp) -> R {
    let k = ip.pop()?;
    key_cost(ip, &k)?;
    match ip.pop()? {
        Obj::Dict(d) => {
            let Some(v) = d.get(&k.key()) else {
                ip.dollar_error.put_name(b"offending", k);
                return err("undefined");
            };
            ip.push(v)
        }
        Obj::Array(a, _) => {
            let i = usize::try_from(int_of(&k)?).map_err(|_| PsErr::Ps("rangecheck"))?;
            let v = a.get(i).ok_or(PsErr::Ps("rangecheck"))?;
            ip.push(v)
        }
        Obj::Str(s, _) => {
            let i = usize::try_from(int_of(&k)?).map_err(|_| PsErr::Ps("rangecheck"))?;
            let v = s.get(i).ok_or(PsErr::Ps("rangecheck"))?;
            ip.push(Obj::Int(i64::from(v)))
        }
        _ => err("typecheck"),
    }
}

fn int_of(o: &Obj) -> R<i64> {
    match o {
        Obj::Int(i) => Ok(*i),
        _ => err("typecheck"),
    }
}

pub fn put(ip: &mut Interp) -> R {
    let v = ip.pop()?;
    let k = ip.pop()?;
    match ip.pop()? {
        Obj::Dict(d) => {
            key_cost(ip, &k)?;
            ip.alloc(2 * OBJ_BYTES)?;
            d.put(k, v);
            Ok(())
        }
        Obj::Array(a, _) => {
            let i = usize::try_from(int_of(&k)?).map_err(|_| PsErr::Ps("rangecheck"))?;
            if a.set(i, v) { Ok(()) } else { err("rangecheck") }
        }
        Obj::Str(s, _) => {
            let i = usize::try_from(int_of(&k)?).map_err(|_| PsErr::Ps("rangecheck"))?;
            let b = u8::try_from(int_of(&v)? & 0xFF).unwrap_or(0);
            if s.set(i, b) { Ok(()) } else { err("rangecheck") }
        }
        _ => err("typecheck"),
    }
}

pub fn known(ip: &mut Interp) -> R {
    let k = ip.pop()?;
    key_cost(ip, &k)?;
    let d = ip.pop_dict()?;
    ip.push(Obj::Bool(d.known(&k.key())))
}

pub fn undef(ip: &mut Interp) -> R {
    let k = ip.pop()?;
    key_cost(ip, &k)?;
    let d = ip.pop_dict()?;
    d.remove(&k.key());
    Ok(())
}

pub fn where_(ip: &mut Interp) -> R {
    let k = ip.pop()?;
    key_cost(ip, &k)?;
    ip.charge(ip.d.len() as u64 / 16)?;
    let k = k.key();
    match ip.d.iter().rev().find(|d| d.known(&k)).cloned() {
        Some(d) => {
            ip.push(Obj::Dict(d))?;
            ip.push(Obj::Bool(true))
        }
        None => ip.push(Obj::Bool(false)),
    }
}

pub fn currentdict(ip: &mut Interp) -> R {
    let d = ip.current_dict()?;
    ip.push(Obj::Dict(d))
}

pub fn countdictstack(ip: &mut Interp) -> R {
    let n = ip.d.len() as i64;
    ip.push(Obj::Int(n))
}

pub fn dictstack(ip: &mut Interp) -> R {
    let a = ip.pop_arr()?;
    let n = ip.d.len().min(a.len);
    for (i, d) in ip.d.iter().take(n).enumerate() {
        a.set(i, Obj::Dict(d.clone()));
    }
    let sub = a.interval(0, n).ok_or(PsErr::Ps("rangecheck"))?;
    ip.push(Obj::Array(sub, false))
}

pub fn cleardictstack(ip: &mut Interp) -> R {
    ip.d.truncate(3);
    Ok(())
}

// ----- strings and arrays -----

pub fn array(ip: &mut Interp) -> R {
    let n = ip.pop_usize()?;
    ip.alloc(n.saturating_mul(OBJ_BYTES))?;
    ip.charge(n as u64)?;
    ip.push(Obj::Array(PArr::new(vec![Obj::Null; n]), false))
}

pub fn string(ip: &mut Interp) -> R {
    let n = ip.pop_usize()?;
    ip.alloc(n)?;
    ip.charge_bytes(n)?;
    ip.push(Obj::Str(PStr::new(vec![0; n]), false))
}

pub fn length(ip: &mut Interp) -> R {
    let n = match ip.pop()? {
        Obj::Array(a, _) => a.len,
        Obj::Str(s, _) => s.len,
        Obj::Dict(d) => d.len(),
        Obj::Name(n, _) => n.len(),
        _ => return err("typecheck"),
    };
    ip.push(Obj::Int(n as i64))
}

pub fn getinterval(ip: &mut Interp) -> R {
    let count = ip.pop_usize()?;
    let start = ip.pop_usize()?;
    match ip.pop()? {
        Obj::Array(a, x) => {
            let s = a.interval(start, count).ok_or(PsErr::Ps("rangecheck"))?;
            ip.push(Obj::Array(s, x))
        }
        Obj::Str(s, x) => {
            let s = s.interval(start, count).ok_or(PsErr::Ps("rangecheck"))?;
            ip.push(Obj::Str(s, x))
        }
        _ => err("typecheck"),
    }
}

pub fn putinterval(ip: &mut Interp) -> R {
    let src = ip.pop()?;
    let start = ip.pop_usize()?;
    match (ip.pop()?, src) {
        (Obj::Array(dst, _), Obj::Array(src, _)) => {
            if start.checked_add(src.len).is_none_or(|e| e > dst.len) {
                return err("rangecheck");
            }
            ip.charge(src.len as u64)?;
            ip.alloc(src.len.saturating_mul(OBJ_BYTES))?;
            for (i, o) in src.items().into_iter().enumerate() {
                dst.set(start + i, o);
            }
            Ok(())
        }
        (Obj::Str(dst, _), Obj::Str(src, _)) => {
            ip.charge_bytes(src.len)?;
            ip.alloc(src.len)?;
            dst.put_bytes(start, &src.bytes()).map(|_| ()).ok_or(PsErr::Ps("rangecheck"))
        }
        _ => err("typecheck"),
    }
}

pub fn aload(ip: &mut Interp) -> R {
    let o = ip.pop()?;
    let Obj::Array(a, _) = &o else { return err("typecheck") };
    ip.charge(a.len as u64)?;
    ip.alloc(a.len.saturating_mul(OBJ_BYTES))?;
    for v in a.items() {
        ip.push(v)?;
    }
    ip.push(o)
}

pub fn astore(ip: &mut Interp) -> R {
    let o = ip.pop()?;
    let Obj::Array(a, _) = &o else { return err("typecheck") };
    ip.need(a.len)?;
    ip.charge(a.len as u64)?;
    let start = ip.o.len() - a.len;
    let items = ip.o.split_off(start);
    for (i, v) in items.into_iter().enumerate() {
        a.set(i, v);
    }
    ip.push(o)
}

pub fn packedarray(ip: &mut Interp) -> R {
    let n = ip.pop_usize()?;
    ip.need(n)?;
    ip.charge(n as u64)?;
    ip.alloc(n.saturating_mul(OBJ_BYTES))?;
    let start = ip.o.len() - n;
    let items = ip.o.split_off(start);
    ip.push(Obj::Array(PArr::new(items), false))
}

pub fn search(ip: &mut Interp) -> R {
    let seek = ip.pop_str()?.bytes();
    let s = ip.pop_str()?;
    ip.charge_bytes(s.len.saturating_add(seek.len()))?;
    ip.alloc(s.len.saturating_add(seek.len()))?;
    let hay = s.bytes();
    let found = find(ip, &hay, &seek)?;
    match found {
        Some(i) => {
            let post = s.interval(i + seek.len(), hay.len() - i - seek.len()).ok_or(PsErr::Ps("rangecheck"))?;
            let m = s.interval(i, seek.len()).ok_or(PsErr::Ps("rangecheck"))?;
            let pre = s.interval(0, i).ok_or(PsErr::Ps("rangecheck"))?;
            ip.push(Obj::Str(post, false))?;
            ip.push(Obj::Str(m, false))?;
            ip.push(Obj::Str(pre, false))?;
            ip.push(Obj::Bool(true))
        }
        None => {
            ip.push(Obj::Str(s, false))?;
            ip.push(Obj::Bool(false))
        }
    }
}

/// Where `seek` first occurs in `hay`, charging the bytes compared as it goes (a naive search
/// costs |hay| × |seek| on inputs made to defeat it).
fn find(ip: &mut Interp, hay: &[u8], seek: &[u8]) -> R<Option<usize>> {
    if seek.is_empty() {
        return Ok(Some(0));
    }
    let mut compared = 0usize;
    for (i, w) in hay.windows(seek.len()).enumerate() {
        let same = w.iter().zip(seek).take_while(|(a, b)| a == b).count();
        compared += same + 1;
        if same == seek.len() {
            ip.charge_bytes(compared)?;
            return Ok(Some(i));
        }
        if compared >= 1 << 16 {
            ip.charge_bytes(compared)?;
            compared = 0;
        }
    }
    ip.charge_bytes(compared)?;
    Ok(None)
}

pub fn anchorsearch(ip: &mut Interp) -> R {
    let seek = ip.pop_str()?.bytes();
    let s = ip.pop_str()?;
    ip.charge_bytes(s.len.saturating_add(seek.len()))?;
    ip.alloc(s.len.saturating_add(seek.len()))?;
    if s.bytes().starts_with(&seek) {
        let post = s.interval(seek.len(), s.len - seek.len()).ok_or(PsErr::Ps("rangecheck"))?;
        let m = s.interval(0, seek.len()).ok_or(PsErr::Ps("rangecheck"))?;
        ip.push(Obj::Str(post, false))?;
        ip.push(Obj::Str(m, false))?;
        ip.push(Obj::Bool(true))
    } else {
        ip.push(Obj::Str(s, false))?;
        ip.push(Obj::Bool(false))
    }
}

/// `token` on a string: the first object and the rest.
pub fn token(ip: &mut Interp) -> R {
    match ip.pop()? {
        Obj::Str(s, _) => {
            ip.alloc(crate::interp::SOURCE_BYTES)?;
            let id = ip.srcs.add(crate::source::Source::string(&s));
            let d = &ip.d;
            let mut resolve = |n: &[u8]| d.iter().rev().find_map(|dict| dict.get(&Key::Name(name(n))));
            match crate::scan::next(&mut ip.srcs, id, &mut resolve) {
                Ok(Some(o)) => {
                    let used = ip.srcs.position(id).unwrap_or(s.len).min(s.len);
                    let rest = s.interval(used, s.len - used).ok_or(PsErr::Ps("rangecheck"))?;
                    ip.push(Obj::Str(rest, false))?;
                    ip.push(o)?;
                    ip.push(Obj::Bool(true))
                }
                Ok(None) => ip.push(Obj::Bool(false)),
                Err(_) => err("syntaxerror"),
            }
        }
        Obj::File(id) => {
            let d = &ip.d;
            let mut resolve = |n: &[u8]| d.iter().rev().find_map(|dict| dict.get(&Key::Name(name(n))));
            match crate::scan::next(&mut ip.srcs, id, &mut resolve) {
                Ok(Some(o)) => {
                    ip.push(o)?;
                    ip.push(Obj::Bool(true))
                }
                Ok(None) => ip.push(Obj::Bool(false)),
                Err(_) => err("syntaxerror"),
            }
        }
        _ => err("typecheck"),
    }
}

/// `bind`: executable names that name operators become the operators, through nested
/// procedures (each procedure once).
pub fn bind(ip: &mut Interp) -> R {
    let o = ip.top(0)?.clone();
    let Obj::Array(root, _) = o else { return Ok(()) };
    let mut todo = vec![root];
    let mut seen: std::collections::HashSet<usize> = std::collections::HashSet::new();
    while let Some(a) = todo.pop() {
        let id = std::rc::Rc::as_ptr(&a.buf) as *const u8 as usize;
        if seen.len() > 100_000 || !seen.insert(id) {
            continue;
        }
        ip.charge(a.len as u64)?;
        for i in 0..a.len {
            match a.get(i) {
                Some(Obj::Name(n, true)) => {
                    if let Some(Obj::Op(op)) = ip.lookup(&Key::Name(n)) {
                        a.set(i, Obj::Op(op));
                    }
                }
                Some(Obj::Array(sub, true)) => todo.push(sub),
                _ => {}
            }
        }
    }
    Ok(())
}
