//! Files, filters, resources, forms, output, and device parameters EPS programs query or set.

use crate::interp::{Interp, PsErr, R, err, matrix_of};
use crate::obj::{Key, Obj, PArr, PDict, name};
use crate::source::{FilterParams, Source};

// ----- files -----

pub fn currentfile(ip: &mut Interp) -> R {
    let id =
        ip.e.iter()
            .rev()
            .find_map(|f| match f {
                crate::interp::Frame::File { id } => Some(*id),
                _ => None,
            })
            .unwrap_or(ip.main);
    ip.push(Obj::File(id))
}

fn pop_file(ip: &mut Interp) -> R<usize> {
    match ip.pop()? {
        Obj::File(id) => Ok(id),
        _ => err("typecheck"),
    }
}

pub fn read(ip: &mut Interp) -> R {
    let f = pop_file(ip)?;
    match ip.srcs.read(f) {
        Some(c) => {
            ip.push(Obj::Int(i64::from(c)))?;
            ip.push(Obj::Bool(true))
        }
        None => ip.push(Obj::Bool(false)),
    }
}

pub fn readstring(ip: &mut Interp) -> R {
    let s = ip.pop_str()?;
    let f = pop_file(ip)?;
    let mut n = 0;
    while n < s.len {
        match ip.srcs.read(f) {
            Some(c) => {
                s.set(n, c);
                n += 1;
            }
            None => break,
        }
    }
    let full = n == s.len;
    let sub = s.interval(0, n).ok_or(PsErr::Ps("rangecheck"))?;
    ip.push(Obj::Str(sub, false))?;
    ip.push(Obj::Bool(full))
}

pub fn readhexstring(ip: &mut Interp) -> R {
    let s = ip.pop_str()?;
    let f = pop_file(ip)?;
    let mut n = 0;
    let mut hi: Option<u8> = None;
    while n < s.len {
        let Some(c) = ip.srcs.read(f) else { break };
        let v = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => continue,
        };
        match hi.take() {
            Some(h) => {
                s.set(n, h << 4 | v);
                n += 1;
            }
            None => hi = Some(v),
        }
    }
    let full = n == s.len;
    let sub = s.interval(0, n).ok_or(PsErr::Ps("rangecheck"))?;
    ip.push(Obj::Str(sub, false))?;
    ip.push(Obj::Bool(full))
}

pub fn readline(ip: &mut Interp) -> R {
    let s = ip.pop_str()?;
    let f = pop_file(ip)?;
    let mut n = 0;
    let mut eol = false;
    while let Some(c) = ip.srcs.read(f) {
        if c == b'\n' {
            eol = true;
            break;
        }
        if c == b'\r' {
            if ip.srcs.peek(f) == Some(b'\n') {
                ip.srcs.read(f);
            }
            eol = true;
            break;
        }
        if n >= s.len {
            return err("rangecheck");
        }
        s.set(n, c);
        n += 1;
    }
    let sub = s.interval(0, n).ok_or(PsErr::Ps("rangecheck"))?;
    ip.push(Obj::Str(sub, false))?;
    ip.push(Obj::Bool(eol || n > 0))
}

pub fn bytesavailable(ip: &mut Interp) -> R {
    let f = pop_file(ip)?;
    let n = ip.srcs.available(f);
    ip.push(Obj::Int(n))
}

pub fn closefile(ip: &mut Interp) -> R {
    let f = pop_file(ip)?;
    ip.srcs.close(f);
    Ok(())
}

pub fn resetfile(ip: &mut Interp) -> R {
    let f = pop_file(ip)?;
    ip.srcs.reset(f);
    Ok(())
}

pub fn fileposition(ip: &mut Interp) -> R {
    let f = pop_file(ip)?;
    let p = ip.srcs.position(f).ok_or(PsErr::Ps("ioerror"))?;
    ip.push(Obj::Int(i64::try_from(p).unwrap_or(i64::MAX)))
}

pub fn setfileposition(ip: &mut Interp) -> R {
    let p = ip.pop_usize()?;
    let f = pop_file(ip)?;
    if ip.srcs.set_position(f, p) { Ok(()) } else { err("ioerror") }
}

pub fn flushfile(ip: &mut Interp) -> R {
    // On an input file: read and discard to its end.
    let f = pop_file(ip)?;
    if f != ip.main {
        while ip.srcs.read(f).is_some() {}
    }
    Ok(())
}

pub fn status(ip: &mut Interp) -> R {
    match ip.pop()? {
        Obj::File(f) => {
            let open = ip.srcs.available(f) >= 0;
            ip.push(Obj::Bool(open))
        }
        _ => ip.push(Obj::Bool(false)),
    }
}

pub fn file(ip: &mut Interp) -> R {
    let _access = ip.pop()?;
    let fname = ip.pop_name()?;
    match fname.as_slice() {
        b"%stdin" => ip.push(Obj::File(ip.main)),
        // Output files swallow what is written.
        b"%stdout" | b"%stderr" | b"%lineedit" | b"%statementedit" => {
            let id = ip.srcs.add(Source::bytes(Vec::new()));
            ip.push(Obj::File(id))
        }
        // An EPS can't reach the disk.
        _ => err("invalidfileaccess"),
    }
}

pub fn write_ops(ip: &mut Interp) -> R {
    // `file int write`, `file string writestring`: discarded.
    ip.pop()?;
    ip.pop()?;
    Ok(())
}

pub fn eexec(ip: &mut Interp) -> R {
    let up = match ip.pop()? {
        Obj::File(id) => id,
        Obj::Str(s, _) => ip.srcs.add(Source::bytes(s.bytes())),
        _ => return err("typecheck"),
    };
    let id = ip.srcs.add(Source::eexec(up));
    // eexec runs with systemdict on top, which its program removes with `end` (`closefile`).
    let sd = ip.systemdict.clone();
    ip.begin(sd)?;
    ip.push_frame(crate::interp::Frame::File { id })
}

/// `src [params] /Name filter`, with SubFileDecode's own `count string` operands.
pub fn filter(ip: &mut Interp) -> R {
    let fname = ip.pop_name()?;
    let mut params = FilterParams { early_change: true, ..Default::default() };
    if fname == b"SubFileDecode" {
        let eod = ip.pop()?;
        params.eod_string = match eod {
            Obj::Str(s, _) => s.bytes(),
            _ => return err("typecheck"),
        };
        params.eod_count = ip.pop_usize()?;
        if let Ok(Obj::Dict(_)) = ip.top(0) {
            ip.pop()?;
        }
    } else if let Ok(Obj::Dict(d)) = ip.top(0).cloned() {
        // A dictionary operand is parameters unless it is the data source itself (never: a
        // dictionary can't be a source).
        ip.pop()?;
        let num = |k: &[u8]| d.get_name(k).and_then(|o| o.num());
        params.early_change = num(b"EarlyChange").is_none_or(|v| v != 0.0);
        params.predictor = num(b"Predictor").unwrap_or(1.0) as i64;
        params.colors = num(b"Colors").unwrap_or(1.0) as i64;
        params.bpc = num(b"BitsPerComponent").unwrap_or(8.0) as i64;
        params.columns = num(b"Columns").unwrap_or(1.0) as i64;
    }
    let src = ip.pop()?;
    let up = match src {
        Obj::File(id) => id,
        Obj::Str(s, _) => ip.srcs.add(Source::bytes(s.bytes())),
        p @ Obj::Array(_, true) => {
            // A procedure source: call it until it gives an empty string.
            let mut data = Vec::new();
            for _ in 0..1_000_000 {
                ip.call(p.clone())?;
                let s = ip.pop_str()?.bytes();
                if s.is_empty() {
                    break;
                }
                data.extend_from_slice(&s);
                ip.alloc(s.len())?;
            }
            ip.srcs.add(Source::bytes(data))
        }
        _ => return err("typecheck"),
    };
    if fname.ends_with(b"Encode") && fname != b"NullEncode" {
        // Encoding filters write; an EPS has nowhere to write to.
        let id = ip.srcs.add(Source::bytes(Vec::new()));
        return ip.push(Obj::File(id));
    }
    match ip.srcs.filter(up, &fname, &params) {
        Ok(id) => ip.push(Obj::File(id)),
        Err(e) => {
            ip.warnings.push(e);
            err("ioerror")
        }
    }
}

// ----- output -----

pub fn pop1(ip: &mut Interp) -> R {
    ip.pop().map(|_| ())
}

pub fn noop(_ip: &mut Interp) -> R {
    Ok(())
}

pub fn showpage(_ip: &mut Interp) -> R {
    Ok(())
}

// ----- environment -----

pub fn languagelevel(ip: &mut Interp) -> R {
    ip.push(Obj::Int(3))
}

pub fn version(ip: &mut Interp) -> R {
    ip.push(Obj::str(b"3010"))
}

pub fn product(ip: &mut Interp) -> R {
    ip.push(Obj::str(b"DesignCraft"))
}

pub fn revision(ip: &mut Interp) -> R {
    ip.push(Obj::Int(1))
}

pub fn zero(ip: &mut Interp) -> R {
    ip.push(Obj::Int(0))
}

pub fn vmstatus(ip: &mut Interp) -> R {
    let used = i64::try_from(ip.memory).unwrap_or(i64::MAX);
    ip.push(Obj::Int(i64::try_from(ip.saves.len()).unwrap_or(0)))?;
    ip.push(Obj::Int(used))?;
    ip.push(Obj::Int(1 << 30))
}

pub fn push_false(ip: &mut Interp) -> R {
    ip.push(Obj::Bool(false))
}

pub fn empty_dict(ip: &mut Interp) -> R {
    ip.push(Obj::Dict(PDict::new(4)))
}

pub fn pop_dict(ip: &mut Interp) -> R {
    ip.pop_dict().map(|_| ())
}

/// The system and user parameters programs read (all limits generous).
pub fn currentsystemparams(ip: &mut Interp) -> R {
    let d = PDict::new(24);
    for k in [
        "MaxPatternCache",
        "MaxFontCache",
        "MaxFormCache",
        "MaxScreenStorage",
        "MaxDisplayList",
        "MaxOutlineCache",
        "MaxUPathCache",
        "MaxStoredScreenCache",
        "MaxSourceList",
        "MaxDisplayAndSourceList",
        "MaxImageBuffer",
        "MaxRasterMemory",
        "MaxStackSize",
        "CurFontCache",
        "CurFormCache",
        "CurPatternCache",
        "CurOutlineCache",
        "CurUPathCache",
        "CurScreenStorage",
        "CurDisplayList",
        "CurSourceList",
        "CurStoredScreenCache",
        "CurInputDevice",
        "CurOutputDevice",
    ] {
        let v = if k.starts_with("Cur") { 0 } else { 8 << 20 };
        d.put_name(k.as_bytes(), Obj::Int(v));
    }
    d.put_name(b"ByteOrder", Obj::Bool(false));
    d.put_name(b"RealFormat", Obj::str(b"IEEE"));
    d.put_name(b"BuildTime", Obj::Int(0));
    ip.push(Obj::Dict(d))
}

pub fn currentuserparams(ip: &mut Interp) -> R {
    let d = PDict::new(20);
    for (k, v) in [
        ("MaxPatternItem", 20_000),
        ("MaxFormItem", 100_000),
        ("MaxFontItem", 12_500),
        ("MaxUPathItem", 0),
        ("MaxLocalVM", 1 << 30),
        ("MaxGlobalVM", 1 << 30),
        ("VMReclaim", 0),
        ("VMThreshold", 1 << 20),
        ("MaxDictStack", 1000),
        ("MaxExecStack", 100_000),
        ("MaxOpStack", 500_000),
        ("MinFontCompress", 100),
        ("MaxSuperScreen", 1016),
        ("MaxLocalVM", 1 << 30),
        ("JobTimeout", 0),
        ("WaitTimeout", 0),
    ] {
        d.put_name(k.as_bytes(), Obj::Int(v));
    }
    d.put_name(b"AccurateScreens", Obj::Bool(false));
    d.put_name(b"HalftoneMode", Obj::Int(0));
    d.put_name(b"IdiomRecognition", Obj::Bool(false));
    ip.push(Obj::Dict(d))
}

pub fn currentpagedevice(ip: &mut Interp) -> R {
    let d = PDict::new(8);
    d.put_name(b"PageSize", Obj::Array(PArr::new(vec![Obj::Int(612), Obj::Int(792)]), false));
    d.put_name(b"HWResolution", Obj::Array(PArr::new(vec![Obj::Int(72), Obj::Int(72)]), false));
    d.put_name(b"Policies", Obj::Dict(PDict::new(2)));
    d.put_name(b"ProcessColorModel", Obj::lit(b"DeviceCMYK"));
    d.put_name(b"Separations", Obj::Bool(false));
    ip.push(Obj::Dict(d))
}

pub fn currentscreen(ip: &mut Interp) -> R {
    ip.push(Obj::Real(60.0))?;
    ip.push(Obj::Real(45.0))?;
    ip.push(Obj::proc(vec![Obj::Op(crate::ops::op_id(b"pop").unwrap_or(0)), Obj::Real(0.0)]))
}

pub fn setscreen(ip: &mut Interp) -> R {
    for _ in 0..3 {
        ip.pop()?;
    }
    Ok(())
}

pub fn setcolorscreen(ip: &mut Interp) -> R {
    for _ in 0..12 {
        ip.pop()?;
    }
    Ok(())
}

pub fn currentcolorscreen(ip: &mut Interp) -> R {
    for _ in 0..4 {
        currentscreen(ip)?;
    }
    Ok(())
}

pub fn empty_proc(ip: &mut Interp) -> R {
    ip.push(Obj::proc(Vec::new()))
}

pub fn currentcolortransfer(ip: &mut Interp) -> R {
    for _ in 0..4 {
        empty_proc(ip)?;
    }
    Ok(())
}

pub fn setcolortransfer(ip: &mut Interp) -> R {
    for _ in 0..4 {
        ip.pop()?;
    }
    Ok(())
}

pub fn findcolorrendering(ip: &mut Interp) -> R {
    let n = ip.pop()?;
    ip.push(n)?;
    ip.push(Obj::Bool(true))
}

pub fn realtime(ip: &mut Interp) -> R {
    ip.push(Obj::Int(0))
}

pub fn internaldict(ip: &mut Interp) -> R {
    ip.pop_int()?;
    let d = match ip.systemdict.get_name(b"DesignCraftInternal") {
        Some(Obj::Dict(d)) => d,
        _ => {
            let d = PDict::new(10);
            ip.systemdict.put_name(b"DesignCraftInternal", Obj::Dict(d.clone()));
            d
        }
    };
    ip.push(Obj::Dict(d))
}

pub fn setglobal(ip: &mut Interp) -> R {
    ip.pop_bool().map(|_| ())
}

pub fn print_any(ip: &mut Interp) -> R {
    ip.pop().map(|_| ())
}

pub fn pstack(_ip: &mut Interp) -> R {
    Ok(())
}

pub fn nulldevice(ip: &mut Interp) -> R {
    ip.gs.nulldev = true;
    Ok(())
}

pub fn execform(ip: &mut Interp) -> R {
    let d = ip.pop_dict()?;
    let m = match d.get_name(b"Matrix") {
        Some(Obj::Array(a, _)) => matrix_of(&a)?,
        _ => crate::gstate::M::ID,
    };
    let paint = d.get_name(b"PaintProc").ok_or(PsErr::Ps("undefined"))?;
    ip.gsave()?;
    ip.gs.ctm = m.then(&ip.gs.ctm);
    let r = (|| {
        if let Some(Obj::Array(bb, _)) = d.get_name(b"BBox") {
            ip.push(Obj::Array(bb, false))?;
            crate::ops::gfx::rectclip(ip)?;
        }
        ip.push(Obj::Dict(d.clone()))?;
        ip.call(paint)
    })();
    ip.grestore();
    r
}

// ----- resources -----

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

pub fn findresource(ip: &mut Interp) -> R {
    let cat = ip.pop_name()?;
    let key = ip.pop()?;
    if cat == b"Font" {
        let f = crate::font::find_font(ip, &key);
        return ip.push(Obj::Dict(f));
    }
    if cat == b"Category" {
        let d = category(ip, &crate::ops::basic::text_of(&key));
        return ip.push(Obj::Dict(d));
    }
    let d = category(ip, &cat);
    if let Some(v) = d.get(&key.key()) {
        return ip.push(v);
    }
    if cat == b"Encoding"
        && let Some(e) = ip.systemdict.get(&key.key())
    {
        return ip.push(e);
    }
    if cat == b"ColorSpace" || cat == b"ColorRendering" || cat == b"Halftone" {
        return ip.push(Obj::Dict(PDict::new(1)));
    }
    ip.dollar_error.put_name(b"command", Obj::lit(b"findresource"));
    err("undefinedresource")
}

pub fn defineresource(ip: &mut Interp) -> R {
    let cat = ip.pop_name()?;
    let v = ip.pop()?;
    let key = ip.pop()?;
    if cat == b"CIDFont"
        && let Obj::Dict(d) = &v
        && !d.known(&Key::Name(name(b"FID")))
    {
        d.put_name(b"FID", Obj::FontId(ip.next_fid));
        ip.next_fid += 1;
    }
    if cat == b"Font"
        && let Obj::Dict(d) = &v
    {
        if !d.known(&Key::Name(name(b"FID"))) {
            d.put_name(b"FID", Obj::FontId(ip.next_fid));
            ip.next_fid += 1;
        }
        ip.fontdir.put(key.clone(), v.clone());
    }
    category(ip, &cat).put(key, v.clone());
    ip.push(v)
}

pub fn undefineresource(ip: &mut Interp) -> R {
    let cat = ip.pop_name()?;
    let key = ip.pop()?;
    category(ip, &cat).remove(&key.key());
    Ok(())
}

pub fn resourcestatus(ip: &mut Interp) -> R {
    let cat = ip.pop_name()?;
    let key = ip.pop()?;
    let known = category(ip, &cat).known(&key.key()) || (cat == b"Font" && ip.fontdir.known(&key.key()));
    if known {
        ip.push(Obj::Int(1))?;
        ip.push(Obj::Int(0))?;
        ip.push(Obj::Bool(true))
    } else {
        ip.push(Obj::Bool(false))
    }
}

pub fn resourceforall(ip: &mut Interp) -> R {
    let cat = ip.pop_name()?;
    let scratch = ip.pop_str()?;
    let proc = ip.pop()?;
    let _template = ip.pop()?;
    let entries = category(ip, &cat).entries();
    ip.charge(entries.len() as u64)?;
    ip.alloc(entries.len().saturating_mul(2 * crate::ops::basic::OBJ_BYTES))?;
    for (k, _) in entries {
        let t = crate::ops::basic::text_of(&k);
        if let Some(w) = scratch.put_bytes(0, &t) {
            ip.push(Obj::Str(w, false))?;
            ip.call(proc.clone())?;
        }
    }
    Ok(())
}

// ----- user paths and objects -----

pub fn ucache(_ip: &mut Interp) -> R {
    Ok(())
}

/// Run a user path (an array of numbers and operator names) to build the current path.
fn user_path(ip: &mut Interp, o: Obj) -> R {
    let items = match o {
        Obj::Array(a, _) => a.items(),
        _ => return err("typecheck"),
    };
    crate::ops::gfx::newpath(ip)?;
    for it in items {
        match it {
            Obj::Name(n, _) => {
                let n: &[u8] = &n;
                if matches!(n, b"ucache" | b"setbbox") {
                    if n == b"setbbox" {
                        for _ in 0..4 {
                            ip.pop()?;
                        }
                    }
                    continue;
                }
                let op = crate::ops::op_id(n).ok_or(PsErr::Ps("typecheck"))?;
                ip.call_op(op)?;
            }
            Obj::Op(op) => {
                if crate::ops::OPS.get(op as usize).is_some_and(|(n, _)| *n == b"setbbox") {
                    for _ in 0..4 {
                        ip.pop()?;
                    }
                    continue;
                }
                ip.call_op(op)?;
            }
            o => ip.push(o)?,
        }
    }
    Ok(())
}

pub fn uappend(ip: &mut Interp) -> R {
    let o = ip.pop()?;
    let saved = std::mem::take(&mut ip.gs.path);
    user_path(ip, o)?;
    let mut p = saved;
    p.append(&mut ip.gs.path);
    ip.gs.path = p;
    Ok(())
}

fn upaint(ip: &mut Interp, op: &[u8]) -> R {
    let mut m = None;
    if let Ok(Obj::Array(a, _)) = ip.top(0).cloned()
        && a.len == 6
        && a.items().iter().all(|o| o.num().is_some())
        && ip.o.len() >= 2
        && matches!(ip.top(1), Ok(Obj::Array(..)))
    {
        m = Some(ip.pop_matrix()?);
    }
    let o = ip.pop()?;
    ip.gsave()?;
    let r = (|| {
        user_path(ip, o)?;
        if let Some(m) = m {
            ip.gs.ctm = m.then(&ip.gs.ctm);
        }
        match op {
            b"ufill" => crate::ops::gfx::fill(ip),
            b"ueofill" => crate::ops::gfx::eofill(ip),
            _ => crate::ops::gfx::stroke(ip),
        }
    })();
    ip.grestore();
    r
}

pub fn ufill(ip: &mut Interp) -> R {
    upaint(ip, b"ufill")
}

pub fn ueofill(ip: &mut Interp) -> R {
    upaint(ip, b"ueofill")
}

pub fn ustroke(ip: &mut Interp) -> R {
    upaint(ip, b"ustroke")
}

pub fn setbbox(ip: &mut Interp) -> R {
    for _ in 0..4 {
        ip.pop_num()?;
    }
    Ok(())
}

pub fn defineuserobject(ip: &mut Interp) -> R {
    let v = ip.pop()?;
    let i = ip.pop_usize()?;
    let arr = match ip.userdict.get_name(b"UserObjects") {
        Some(Obj::Array(a, _)) if a.len > i => a,
        old => {
            let mut items = match old {
                Some(Obj::Array(a, _)) => a.items(),
                _ => Vec::new(),
            };
            if i >= 100_000 {
                return err("limitcheck");
            }
            ip.charge((i + 1) as u64)?;
            ip.alloc((i + 1).saturating_mul(crate::ops::basic::OBJ_BYTES))?;
            items.resize(i + 1, Obj::Null);
            let a = PArr::new(items);
            ip.userdict.put_name(b"UserObjects", Obj::Array(a.clone(), false));
            a
        }
    };
    arr.set(i, v);
    Ok(())
}

pub fn execuserobject(ip: &mut Interp) -> R {
    let i = ip.pop_usize()?;
    let v = match ip.userdict.get_name(b"UserObjects") {
        Some(Obj::Array(a, _)) => a.get(i).ok_or(PsErr::Ps("undefined"))?,
        _ => return err("undefined"),
    };
    ip.exec_value(v)
}

pub fn undefineuserobject(ip: &mut Interp) -> R {
    ip.pop_usize().map(|_| ())
}

pub fn string_noop(ip: &mut Interp) -> R {
    // `setsmoothness` and the like: one number.
    ip.pop_num().map(|_| ())
}

pub fn gcheck(ip: &mut Interp) -> R {
    ip.pop()?;
    ip.push(Obj::Bool(false))
}
