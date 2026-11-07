//! Type 1 glyphs: charstrings decrypted and run into outlines (Adobe Type 1 Font Format).

use crate::gstate::Seg;
use crate::obj::{Key, Obj, PDict, name};

/// A glyph outline in glyph space and its advance.
#[derive(Debug, Clone, Default)]
pub struct Glyph {
    pub path: Vec<Seg>,
    pub width: (f64, f64),
    /// The work making it took (charstring operators run, or outline points).
    pub cost: usize,
}

/// The most charstring operators run for one glyph (subroutines included).
const MAX_OPS: usize = 100_000;
const MAX_CALL_DEPTH: usize = 12;

pub fn decrypt(data: &[u8], skip: usize) -> Vec<u8> {
    let mut r: u16 = 4330;
    let mut out = Vec::with_capacity(data.len());
    for &c in data {
        out.push(c ^ (r >> 8) as u8);
        r = (u16::from(c).wrapping_add(r)).wrapping_mul(52845).wrapping_add(22719);
    }
    out.into_iter().skip(skip).collect()
}

fn string_bytes(o: &Obj) -> Option<Vec<u8>> {
    match o {
        Obj::Str(s, _) => Some(s.bytes()),
        _ => None,
    }
}

struct Font {
    charstrings: PDict,
    subrs: Vec<Vec<u8>>,
    len_iv: i64,
}

impl Font {
    fn of(font: &PDict) -> Option<Font> {
        let Some(Obj::Dict(charstrings)) = font.get_name(b"CharStrings") else { return None };
        let private = match font.get_name(b"Private") {
            Some(Obj::Dict(p)) => Some(p),
            _ => None,
        };
        let len_iv = private.as_ref().and_then(|p| p.get_name(b"lenIV")).and_then(|o| o.num()).unwrap_or(4.0) as i64;
        let subrs = match private.as_ref().and_then(|p| p.get_name(b"Subrs")) {
            Some(Obj::Array(a, _)) => a.items().iter().map(|o| string_bytes(o).unwrap_or_default()).collect(),
            _ => Vec::new(),
        };
        Some(Font { charstrings, subrs, len_iv })
    }

    fn plain(&self, data: &[u8]) -> Vec<u8> {
        if self.len_iv < 0 { data.to_vec() } else { decrypt(data, usize::try_from(self.len_iv).unwrap_or(4)) }
    }

    fn charstring(&self, glyph: &[u8]) -> Option<Vec<u8>> {
        let o = self.charstrings.get(&Key::Name(name(glyph))).or_else(|| self.charstrings.get(&Key::Name(name(b".notdef"))))?;
        string_bytes(&o).map(|d| self.plain(&d))
    }
}

#[derive(Default)]
struct Run {
    path: Vec<Seg>,
    x: f64,
    y: f64,
    open: bool,
    width: (f64, f64),
    sb: (f64, f64),
    stack: Vec<f64>,
    ps: Vec<f64>,
    flex: Option<Vec<(f64, f64)>>,
    ops: usize,
    done: bool,
}

impl Run {
    fn move_to(&mut self, x: f64, y: f64) {
        self.x = x;
        self.y = y;
        if let Some(pts) = self.flex.as_mut() {
            pts.push((x, y));
            return;
        }
        if self.open {
            self.path.push(Seg::Close);
        }
        self.path.push(Seg::Move(x, y));
        self.open = true;
    }

    fn line_to(&mut self, x: f64, y: f64) {
        if !self.open {
            self.path.push(Seg::Move(self.x, self.y));
            self.open = true;
        }
        self.x = x;
        self.y = y;
        self.path.push(Seg::Line(x, y));
    }

    fn curve_to(&mut self, a: (f64, f64), b: (f64, f64), c: (f64, f64)) {
        if !self.open {
            self.path.push(Seg::Move(self.x, self.y));
            self.open = true;
        }
        self.x = c.0;
        self.y = c.1;
        self.path.push(Seg::Curve(a.0, a.1, b.0, b.1, c.0, c.1));
    }

    fn close(&mut self) {
        if self.open {
            self.path.push(Seg::Close);
            self.open = false;
        }
    }
}

fn exec(font: &Font, code: &[u8], run: &mut Run, depth: usize, std_glyph: &dyn Fn(usize) -> Option<Vec<u8>>) -> Option<()> {
    if depth > MAX_CALL_DEPTH {
        return None;
    }
    let mut i = 0;
    while i < code.len() && !run.done {
        run.ops += 1;
        if run.ops > MAX_OPS || run.stack.len() > 64 {
            return None;
        }
        let v = *code.get(i)?;
        i += 1;
        if v >= 32 {
            let n = match v {
                32..=246 => f64::from(v) - 139.0,
                247..=250 => {
                    let w = *code.get(i)?;
                    i += 1;
                    (f64::from(v) - 247.0) * 256.0 + f64::from(w) + 108.0
                }
                251..=254 => {
                    let w = *code.get(i)?;
                    i += 1;
                    -(f64::from(v) - 251.0) * 256.0 - f64::from(w) - 108.0
                }
                _ => {
                    let b: [u8; 4] = code.get(i..i + 4)?.try_into().ok()?;
                    i += 4;
                    f64::from(i32::from_be_bytes(b))
                }
            };
            run.stack.push(n);
            continue;
        }
        let s = std::mem::take(&mut run.stack);
        let a = |k: usize| s.get(k).copied().unwrap_or(0.0);
        match v {
            // hstem, vstem: hints only.
            1 | 3 => {}
            4 => run.move_to(run.x, run.y + a(0)),
            5 => run.line_to(run.x + a(0), run.y + a(1)),
            6 => run.line_to(run.x + a(0), run.y),
            7 => run.line_to(run.x, run.y + a(0)),
            8 => {
                let p1 = (run.x + a(0), run.y + a(1));
                let p2 = (p1.0 + a(2), p1.1 + a(3));
                let p3 = (p2.0 + a(4), p2.1 + a(5));
                run.curve_to(p1, p2, p3);
            }
            9 => run.close(),
            10 => {
                let n = usize::try_from(a(s.len().saturating_sub(1)) as i64).ok()?;
                // Arguments below the subroutine number stay for it.
                run.stack = s.get(..s.len().saturating_sub(1)).map(<[f64]>::to_vec).unwrap_or_default();
                let sub = font.subrs.get(n)?;
                let plain = font.plain(sub);
                exec(font, &plain, run, depth + 1, std_glyph)?;
                continue;
            }
            11 => {
                run.stack = s;
                return Some(());
            }
            12 => {
                let e = *code.get(i)?;
                i += 1;
                match e {
                    // dotsection, vstem3, hstem3: hints.
                    0..=2 => {}
                    6 => {
                        // seac: an accented character from two StandardEncoding glyphs.
                        let (asb, adx, ady, bchar, achar) = (a(0), a(1), a(2), a(3) as usize, a(4) as usize);
                        let base = font.charstring(&std_glyph(bchar)?)?;
                        let accent = font.charstring(&std_glyph(achar)?)?;
                        let mut b = Run::default();
                        exec(font, &base, &mut b, depth + 1, std_glyph)?;
                        let mut acc = Run::default();
                        exec(font, &accent, &mut acc, depth + 1, std_glyph)?;
                        let (dx, dy) = (adx - asb + run.sb.0, ady);
                        run.path.extend(b.path);
                        run.path.extend(acc.path.iter().map(|s| s.map(&crate::gstate::M([1.0, 0.0, 0.0, 1.0, dx, dy]))));
                        run.done = true;
                    }
                    7 => {
                        run.sb = (a(0), a(1));
                        run.width = (a(2), a(3));
                        run.x = a(0);
                        run.y = a(1);
                    }
                    12 => {
                        let n = s.len();
                        let d = a(n.saturating_sub(1));
                        let q = if d != 0.0 { a(n.saturating_sub(2)) / d } else { 0.0 };
                        run.stack = s.get(..n.saturating_sub(2)).map(<[f64]>::to_vec).unwrap_or_default();
                        run.stack.push(q);
                        continue;
                    }
                    16 => {
                        // callothersubr: flex (0–2) and hint replacement (3) are done here.
                        let n = s.len();
                        let other = a(n.saturating_sub(1)) as i64;
                        let argc = usize::try_from(a(n.saturating_sub(2)) as i64).unwrap_or(0).min(n.saturating_sub(2));
                        let args: Vec<f64> = s.get(n.saturating_sub(2) - argc..n.saturating_sub(2)).map(<[f64]>::to_vec).unwrap_or_default();
                        run.stack = s.get(..n.saturating_sub(2) - argc).map(<[f64]>::to_vec).unwrap_or_default();
                        match other {
                            1 => run.flex = Some(Vec::new()),
                            2 => {}
                            0 => {
                                let pts = run.flex.take().unwrap_or_default();
                                if let (Some(&p1), Some(&p2), Some(&p3), Some(&p4), Some(&p5), Some(&p6)) =
                                    (pts.get(1), pts.get(2), pts.get(3), pts.get(4), pts.get(5), pts.get(6))
                                {
                                    run.curve_to(p1, p2, p3);
                                    run.curve_to(p4, p5, p6);
                                }
                                // The end point for `pop pop setcurrentpoint`.
                                run.ps = vec![run.y, run.x];
                            }
                            _ => {
                                // Hint replacement and others: their arguments come back.
                                run.ps = args.into_iter().rev().collect();
                            }
                        }
                        continue;
                    }
                    17 => {
                        let v = run.ps.pop().unwrap_or(0.0);
                        run.stack = s;
                        run.stack.push(v);
                        continue;
                    }
                    33 => {
                        run.x = a(0);
                        run.y = a(1);
                    }
                    _ => {}
                }
            }
            13 => {
                run.sb = (a(0), 0.0);
                run.width = (a(1), 0.0);
                run.x = a(0);
                run.y = 0.0;
            }
            14 => {
                run.close();
                run.done = true;
            }
            21 => run.move_to(run.x + a(0), run.y + a(1)),
            22 => run.move_to(run.x + a(0), run.y),
            30 => {
                let p1 = (run.x, run.y + a(0));
                let p2 = (p1.0 + a(1), p1.1 + a(2));
                let p3 = (p2.0 + a(3), p2.1);
                run.curve_to(p1, p2, p3);
            }
            31 => {
                let p1 = (run.x + a(0), run.y);
                let p2 = (p1.0 + a(1), p1.1 + a(2));
                let p3 = (p2.0, p2.1 + a(3));
                run.curve_to(p1, p2, p3);
            }
            _ => {}
        }
    }
    Some(())
}

/// The outline and advance of `glyph` in a Type 1 font dictionary, None when it has none.
pub fn glyph(font: &PDict, glyph: &[u8], std_names: &[&str]) -> Option<Glyph> {
    let f = Font::of(font)?;
    let code = f.charstring(glyph)?;
    let mut run = Run::default();
    let std_glyph = |i: usize| std_names.get(i).map(|n| n.as_bytes().to_vec());
    exec(&f, &code, &mut run, 0, &std_glyph)?;
    run.close();
    Some(Glyph { cost: run.ops + run.path.len(), path: run.path, width: run.width })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::obj::{PArr, PStr};

    fn encrypt(plain: &[u8]) -> Vec<u8> {
        let mut r: u16 = 4330;
        let mut out = Vec::new();
        for &p in [0u8; 4].iter().chain(plain) {
            let c = p ^ (r >> 8) as u8;
            r = (u16::from(c).wrapping_add(r)).wrapping_mul(52845).wrapping_add(22719);
            out.push(c);
        }
        out
    }

    fn num(v: i32) -> Vec<u8> {
        match v {
            -107..=107 => vec![(v + 139) as u8],
            108..=1131 => {
                let v = v - 108;
                vec![(v / 256 + 247) as u8, (v % 256) as u8]
            }
            -1131..=-108 => {
                let v = -v - 108;
                vec![(v / 256 + 251) as u8, (v % 256) as u8]
            }
            _ => [vec![255], v.to_be_bytes().to_vec()].concat(),
        }
    }

    #[test]
    fn a_square_glyph_and_its_width() {
        // 50 600 hsbw; 0 0 rmoveto... : a 500 × 700 box through a subroutine.
        let sub = [num(500), vec![6], num(700), vec![7], vec![11]].concat();
        let cs = [num(50), num(600), vec![13], num(0), num(0), vec![21], num(0), vec![10], num(-500), vec![6], vec![9], vec![14]].concat();
        let font = PDict::new(4);
        let chars = PDict::new(2);
        chars.put_name(b"A", Obj::Str(PStr::new(encrypt(&cs)), false));
        font.put_name(b"CharStrings", Obj::Dict(chars));
        let private = PDict::new(2);
        private.put_name(b"Subrs", Obj::Array(PArr::new(vec![Obj::Str(PStr::new(encrypt(&sub)), false)]), false));
        font.put_name(b"Private", Obj::Dict(private));
        let g = glyph(&font, b"A", &[]).unwrap();
        assert_eq!(g.width, (600.0, 0.0));
        assert_eq!(g.path.first(), Some(&Seg::Move(50.0, 0.0)));
        assert!(g.path.contains(&Seg::Line(550.0, 700.0)), "{:?}", g.path);
        // Garbage doesn't panic or loop.
        let chars = PDict::new(1);
        chars.put_name(b"B", Obj::Str(PStr::new(encrypt(&[10, 10, 10, 255, 255])), false));
        font.put_name(b"CharStrings", Obj::Dict(chars));
        let _ = glyph(&font, b"B", &[]);
    }
}
