//! The scanner: turns a file's bytes into objects, building procedures `{ … }` whole.

use crate::obj::{Obj, PArr, PStr, name};
use crate::source::Sources;

/// How deeply procedures may nest.
const MAX_NESTING: usize = 1000;
/// The longest string or name token.
const MAX_TOKEN: usize = 64 << 20;

pub enum Token {
    Obj(Obj),
    /// `//name`: replaced by its value when scanned.
    Immediate(Vec<u8>),
}

pub fn is_ws(c: u8) -> bool {
    matches!(c, 0 | b'\t' | b'\n' | 0x0c | b'\r' | b' ')
}

pub fn is_delim(c: u8) -> bool {
    matches!(c, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

/// One token from `src`, or None at its end. Procedures come back whole, as executable arrays;
/// `//name` inside them is resolved through `resolve`.
pub fn next(srcs: &mut Sources, src: usize, resolve: &mut dyn FnMut(&[u8]) -> Option<Obj>) -> Result<Option<Obj>, String> {
    let mut stack: Vec<Vec<Obj>> = Vec::new();
    loop {
        let Some(tok) = token(srcs, src)? else {
            return if stack.is_empty() { Ok(None) } else { Err("unterminated procedure".into()) };
        };
        let obj = match tok {
            Token::Obj(Obj::Name(n, true)) if &*n == b"{" => {
                if stack.len() >= MAX_NESTING {
                    return Err("procedures nest too deeply".into());
                }
                stack.push(Vec::new());
                continue;
            }
            Token::Obj(Obj::Name(n, true)) if &*n == b"}" => {
                let Some(body) = stack.pop() else { return Err("unmatched }".into()) };
                Obj::Array(PArr::new(body), true)
            }
            Token::Obj(o) => o,
            Token::Immediate(n) => resolve(&n).ok_or_else(|| format!("undefined //{}", String::from_utf8_lossy(&n)))?,
        };
        match stack.last_mut() {
            Some(body) => body.push(obj),
            None => return Ok(Some(obj)),
        }
    }
}

fn token(srcs: &mut Sources, src: usize) -> Result<Option<Token>, String> {
    // Whitespace and comments.
    let c = loop {
        let Some(c) = srcs.read(src) else { return Ok(None) };
        if is_ws(c) {
            continue;
        }
        if c == b'%' {
            while let Some(c) = srcs.read(src) {
                if c == b'\n' || c == b'\r' {
                    break;
                }
            }
            continue;
        }
        break c;
    };
    match c {
        b'(' => string(srcs, src).map(|s| Some(Token::Obj(Obj::Str(PStr::new(s), false)))),
        b'<' => match srcs.read(src) {
            Some(b'<') => Ok(Some(Token::Obj(Obj::Name(name(b"<<"), true)))),
            Some(b'~') => ascii85(srcs, src).map(|s| Some(Token::Obj(Obj::Str(PStr::new(s), false)))),
            Some(c) => {
                srcs.unread(src);
                let _ = c;
                hex(srcs, src).map(|s| Some(Token::Obj(Obj::Str(PStr::new(s), false))))
            }
            None => Err("unterminated hex string".into()),
        },
        b'>' => match srcs.read(src) {
            Some(b'>') => Ok(Some(Token::Obj(Obj::Name(name(b">>"), true)))),
            _ => Err("stray >".into()),
        },
        b'[' | b']' | b'{' | b'}' => Ok(Some(Token::Obj(Obj::Name(name(&[c]), true)))),
        b')' => Err("stray )".into()),
        b'/' => {
            let immediate = srcs.peek(src) == Some(b'/');
            if immediate {
                srcs.read(src);
            }
            let n = regular(srcs, src, None)?;
            Ok(Some(if immediate { Token::Immediate(n) } else { Token::Obj(Obj::Name(name(&n), false)) }))
        }
        _ => {
            let t = regular(srcs, src, Some(c))?;
            Ok(Some(Token::Obj(number(&t).unwrap_or_else(|| Obj::Name(name(&t), true)))))
        }
    }
}

/// A run of regular characters. The whitespace character that ends it is consumed (a CR LF
/// pair as one), a delimiter is left for the next token.
fn regular(srcs: &mut Sources, src: usize, first: Option<u8>) -> Result<Vec<u8>, String> {
    let mut t: Vec<u8> = first.into_iter().collect();
    while let Some(c) = srcs.read(src) {
        if is_ws(c) {
            if c == b'\r' && srcs.peek(src) == Some(b'\n') {
                srcs.read(src);
            }
            break;
        }
        if is_delim(c) {
            srcs.unread(src);
            break;
        }
        t.push(c);
        if t.len() > MAX_TOKEN {
            return Err("token too long".into());
        }
    }
    Ok(t)
}

fn string(srcs: &mut Sources, src: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    loop {
        let c = srcs.read(src).ok_or("unterminated string")?;
        match c {
            b'(' => {
                depth += 1;
                out.push(c);
            }
            b')' => {
                if depth == 0 {
                    return Ok(out);
                }
                depth -= 1;
                out.push(c);
            }
            b'\\' => {
                let e = srcs.read(src).ok_or("unterminated string")?;
                match e {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'\r' => {
                        if srcs.peek(src) == Some(b'\n') {
                            srcs.read(src);
                        }
                    }
                    b'\n' => {}
                    b'0'..=b'7' => {
                        let mut v = u32::from(e - b'0');
                        for _ in 0..2 {
                            match srcs.peek(src) {
                                Some(d @ b'0'..=b'7') => {
                                    srcs.read(src);
                                    v = v * 8 + u32::from(d - b'0');
                                }
                                _ => break,
                            }
                        }
                        out.push((v & 0xFF) as u8);
                    }
                    other => out.push(other),
                }
            }
            // An end of line inside a string is a newline, whatever its form.
            b'\r' => {
                if srcs.peek(src) == Some(b'\n') {
                    srcs.read(src);
                }
                out.push(b'\n');
            }
            _ => out.push(c),
        }
        if out.len() > MAX_TOKEN {
            return Err("string too long".into());
        }
    }
}

fn hex(srcs: &mut Sources, src: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut hi: Option<u8> = None;
    loop {
        let c = srcs.read(src).ok_or("unterminated hex string")?;
        if c == b'>' {
            break;
        }
        if is_ws(c) {
            continue;
        }
        let v = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => return Err("bad hex string".into()),
        };
        match hi.take() {
            Some(h) => out.push(h << 4 | v),
            None => hi = Some(v),
        }
        if out.len() > MAX_TOKEN {
            return Err("string too long".into());
        }
    }
    if let Some(h) = hi {
        out.push(h << 4);
    }
    Ok(out)
}

fn ascii85(srcs: &mut Sources, src: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut group = [0u64; 5];
    let mut n = 0;
    loop {
        let c = srcs.read(src).ok_or("unterminated ASCII85 string")?;
        if is_ws(c) {
            continue;
        }
        if c == b'~' {
            if srcs.read(src) != Some(b'>') {
                return Err("bad ASCII85 end".into());
            }
            break;
        }
        if c == b'z' && n == 0 {
            out.extend([0; 4]);
            continue;
        }
        if !(b'!'..=b'u').contains(&c) {
            return Err("bad ASCII85 string".into());
        }
        if let Some(g) = group.get_mut(n) {
            *g = u64::from(c - b'!');
        }
        n += 1;
        if n == 5 {
            let v = group.iter().fold(0u64, |a, d| a * 85 + d);
            out.extend(u32::try_from(v).map_err(|_| "ASCII85 overflow")?.to_be_bytes());
            n = 0;
        }
        if out.len() > MAX_TOKEN {
            return Err("string too long".into());
        }
    }
    if n > 1 {
        for g in group.iter_mut().skip(n) {
            *g = 84;
        }
        let v = group.iter().fold(0u64, |a, d| a * 85 + d).min(u64::from(u32::MAX));
        let v = u32::try_from(v).unwrap_or(u32::MAX);
        out.extend(v.to_be_bytes().iter().take(n - 1));
    }
    Ok(out)
}

/// An integer, radix number (`16#FF`) or real; None for anything else.
pub fn number(t: &[u8]) -> Option<Obj> {
    let s = std::str::from_utf8(t).ok()?;
    let first = *t.first()?;
    if !(first.is_ascii_digit() || matches!(first, b'+' | b'-' | b'.')) {
        return None;
    }
    if let Some((base, digits)) = s.split_once('#') {
        let base: u32 = base.parse().ok()?;
        if !(2..=36).contains(&base) || digits.is_empty() {
            return None;
        }
        let v = u64::from_str_radix(digits, base).ok()?;
        // Radix numbers are unsigned 32-bit patterns.
        return Some(Obj::Int(i64::from(v as u32 as i32)));
    }
    let digits_only = s.strip_prefix(['+', '-']).unwrap_or(s);
    if !digits_only.is_empty() && digits_only.bytes().all(|b| b.is_ascii_digit()) {
        return Some(match s.parse::<i64>() {
            Ok(i) if i32::try_from(i).is_ok() => Obj::Int(i),
            _ => Obj::Real(s.parse::<f64>().ok()?),
        });
    }
    // Reals: digits with a point and/or an exponent; nothing else (`1e` or `.` alone are names).
    let mut seen_digit = false;
    let mut seen_e = false;
    for (i, b) in digits_only.bytes().enumerate() {
        match b {
            b'0'..=b'9' => seen_digit = true,
            b'.' if !seen_e => {}
            b'e' | b'E' if seen_digit && !seen_e => seen_e = true,
            b'+' | b'-' if seen_e && matches!(digits_only.as_bytes().get(i.wrapping_sub(1)), Some(b'e' | b'E')) => {}
            _ => return None,
        }
    }
    if !seen_digit || digits_only.ends_with(['e', 'E', '+', '-']) {
        return None;
    }
    let v: f64 = s.parse().ok()?;
    v.is_finite().then_some(Obj::Real(v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Source;

    fn all(text: &[u8]) -> Vec<Obj> {
        let mut s = Sources::default();
        let id = s.add(Source::bytes(text.to_vec()));
        let mut out = Vec::new();
        while let Some(o) = next(&mut s, id, &mut |_| Some(Obj::Int(42))).unwrap() {
            out.push(o);
        }
        out
    }

    #[test]
    fn scans_the_token_kinds() {
        let t = all(b"12 -3.5 1e3 16#FF /lit name (a\\(b\\)\\101\\\n c) <41 42> <~87cURDZ~> {1 {2} add} //x [ ] << >> % note\n.5 1.e2 -.");
        let kinds: Vec<String> = t
            .iter()
            .map(|o| match o {
                Obj::Int(i) => format!("i{i}"),
                Obj::Real(r) => format!("r{r}"),
                Obj::Name(n, x) => format!("{}{}", if *x { "x" } else { "/" }, String::from_utf8_lossy(n)),
                Obj::Str(s, _) => format!("s{}", String::from_utf8_lossy(&s.bytes())),
                Obj::Array(a, true) => format!("p{}", a.len),
                _ => "?".into(),
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "i12", "r-3.5", "r1000", "i255", "/lit", "xname", "sa(b)A c", "sAB", "sHello", "p3", "i42", "x[", "x]", "x<<", "x>>", "r0.5", "r100",
                "x-."
            ]
        );
    }

    #[test]
    fn a_token_ending_in_whitespace_consumes_it() {
        let mut s = Sources::default();
        let id = s.add(Source::bytes(b"image\r\nDATA".to_vec()));
        let _ = next(&mut s, id, &mut |_| None).unwrap();
        assert_eq!(s.read(id), Some(b'D'));
    }

    #[test]
    fn malformed_input_is_an_error() {
        for bad in [&b"(open"[..], b"<4g>", b"{ 1 2", b"}", b"<~ab~x"] {
            let mut s = Sources::default();
            let id = s.add(Source::bytes(bad.to_vec()));
            let mut r = Ok(None);
            for _ in 0..5 {
                r = next(&mut s, id, &mut |_| None);
                if r.is_err() {
                    break;
                }
            }
            assert!(r.is_err(), "{}", String::from_utf8_lossy(bad));
        }
        // Deep nesting is refused, not a stack overflow.
        let deep = vec![b'{'; 5000];
        let mut s = Sources::default();
        let id = s.add(Source::bytes(deep));
        assert!(next(&mut s, id, &mut |_| None).is_err());
    }
}
