//! The graphics state: transformation, current path (kept in device space, as PostScript
//! defines it), colour, line parameters and clipping.

use std::rc::Rc;

use crate::obj::{Obj, PDict};

/// An affine matrix `[a b c d e f]`: (x, y) → (a·x + c·y + e, b·x + d·y + f).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct M(pub [f64; 6]);

impl M {
    pub const ID: M = M([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    /// `self` first, then `o` (PostScript's `concat` is `m · CTM`).
    pub fn then(&self, o: &M) -> M {
        let [a, b, c, d, e, f] = self.0;
        let [p, q, r, s, t, u] = o.0;
        M([a * p + b * r, a * q + b * s, c * p + d * r, c * q + d * s, e * p + f * r + t, e * q + f * s + u])
    }

    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }

    pub fn delta(&self, x: f64, y: f64) -> (f64, f64) {
        let [a, b, c, d, ..] = self.0;
        (a * x + c * y, b * x + d * y)
    }

    pub fn inv(&self) -> Option<M> {
        let [a, b, c, d, e, f] = self.0;
        let det = a * d - b * c;
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        let m = M([d / det, -b / det, -c / det, a / det, (c * f - d * e) / det, (b * e - a * f) / det]);
        m.0.iter().all(|v| v.is_finite()).then_some(m)
    }

    pub fn finite(&self) -> bool {
        self.0.iter().all(|v| v.is_finite())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Move(f64, f64),
    Line(f64, f64),
    Curve(f64, f64, f64, f64, f64, f64),
    Close,
}

impl Seg {
    pub fn map(&self, m: &M) -> Seg {
        match *self {
            Seg::Move(x, y) => {
                let (x, y) = m.apply(x, y);
                Seg::Move(x, y)
            }
            Seg::Line(x, y) => {
                let (x, y) = m.apply(x, y);
                Seg::Line(x, y)
            }
            Seg::Curve(a, b, c, d, e, f) => {
                let (a, b) = m.apply(a, b);
                let (c, d) = m.apply(c, d);
                let (e, f) = m.apply(e, f);
                Seg::Curve(a, b, c, d, e, f)
            }
            Seg::Close => Seg::Close,
        }
    }
}

/// The most segments one path may hold.
pub const MAX_PATH: usize = 4_000_000;

/// Bézier segments for an arc of the circle at (cx, cy) with radius r, from angle a0 to a1
/// (degrees), in user space: the first point and the curves.
pub fn arc_curves(cx: f64, cy: f64, r: f64, a0: f64, a1: f64) -> ((f64, f64), Vec<[f64; 6]>) {
    let (a0, a1) = (a0.to_radians(), a1.to_radians());
    let start = (cx + r * a0.cos(), cy + r * a0.sin());
    let sweep = a1 - a0;
    if !sweep.is_finite() || sweep == 0.0 {
        return (start, Vec::new());
    }
    let n = ((sweep.abs() / std::f64::consts::FRAC_PI_2).ceil() as usize).clamp(1, 64);
    let step = sweep / n as f64;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let mut out = Vec::with_capacity(n);
    let mut a = a0;
    for _ in 0..n {
        let b = a + step;
        let (ca, sa, cb, sb) = (a.cos(), a.sin(), b.cos(), b.sin());
        out.push([cx + r * (ca - k * sa), cy + r * (sa + k * ca), cx + r * (cb + k * sb), cy + r * (sb - k * cb), cx + r * cb, cy + r * sb]);
        a = b;
    }
    (start, out)
}

#[derive(Clone, Debug)]
pub enum Space {
    Gray,
    Rgb,
    Cmyk,
    /// Separation or DeviceN: the colourant names, the alternate space and its tint transform.
    Tint {
        names: Vec<Vec<u8>>,
        alt: Box<Space>,
        tint: Obj,
    },
    Indexed {
        base: Box<Space>,
        hival: usize,
        lookup: Vec<u8>,
    },
    Lab,
    Pattern,
}

impl Space {
    pub fn comps(&self) -> usize {
        match self {
            Space::Gray | Space::Indexed { .. } | Space::Pattern => 1,
            Space::Rgb | Space::Lab => 3,
            Space::Cmyk => 4,
            Space::Tint { names, .. } => names.len().max(1),
        }
    }

    pub fn initial(&self) -> Vec<f64> {
        match self {
            Space::Cmyk => vec![0.0, 0.0, 0.0, 1.0],
            Space::Tint { names, .. } => vec![1.0; names.len().max(1)],
            Space::Lab => vec![0.0, 0.0, 0.0],
            _ => vec![0.0; self.comps()],
        }
    }
}

/// A device colour as written to the PDF.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DevColor {
    Gray(f64),
    Rgb(f64, f64, f64),
    Cmyk(f64, f64, f64, f64),
}

/// A clipping path, intersected with the ones before it.
#[derive(Debug)]
pub struct Clip {
    pub path: Vec<Seg>,
    pub eo: bool,
    pub parent: Option<Rc<Clip>>,
    pub depth: usize,
}

pub fn clip_chain(c: &Option<Rc<Clip>>) -> Vec<Rc<Clip>> {
    let mut out = Vec::new();
    let mut cur = c.clone();
    while let Some(n) = cur {
        cur = n.parent.clone();
        out.push(n);
    }
    out.reverse();
    out
}

#[derive(Clone, Debug)]
pub struct GState {
    pub ctm: M,
    pub path: Vec<Seg>,
    /// The current point, in device space.
    pub cur: Option<(f64, f64)>,
    /// Where the current subpath started (closepath returns there).
    pub start: Option<(f64, f64)>,
    pub space: Space,
    pub color: Vec<f64>,
    /// The pattern dictionary when the space is Pattern.
    pub pattern: Option<PDict>,
    pub line_width: f64,
    pub cap: i64,
    pub join: i64,
    pub miter: f64,
    pub dash: (Vec<f64>, f64),
    pub flat: f64,
    pub font: Option<PDict>,
    pub clip: Option<Rc<Clip>>,
    pub overprint: bool,
    pub stroke_adjust: bool,
    /// Clips saved by `clipsave`.
    pub clip_saves: Vec<Option<Rc<Clip>>>,
    /// The colour space under an uncoloured pattern (`[/Pattern /DeviceRGB]`).
    pub pattern_base: Option<Box<Space>>,
    /// Inside an uncoloured pattern's PaintProc: colour operators are ignored.
    pub color_locked: bool,
    /// Painting goes nowhere (`nulldevice`, measuring glyphs).
    pub nulldev: bool,
    /// Glyph widths reported by `setcharwidth`/`setcachedevice` while a glyph runs.
    pub char_width: Option<(f64, f64)>,
}

impl Default for GState {
    fn default() -> Self {
        GState {
            ctm: M::ID,
            path: Vec::new(),
            cur: None,
            start: None,
            space: Space::Gray,
            color: vec![0.0],
            pattern: None,
            line_width: 1.0,
            cap: 0,
            join: 0,
            miter: 10.0,
            dash: (Vec::new(), 0.0),
            flat: 1.0,
            font: None,
            clip: None,
            overprint: false,
            stroke_adjust: false,
            clip_saves: Vec::new(),
            color_locked: false,
            pattern_base: None,
            nulldev: false,
            char_width: None,
        }
    }
}

/// The bounding box of a path's points (control points included), or None for an empty path.
pub fn bbox(path: &[Seg]) -> Option<[f64; 4]> {
    let mut b: Option<[f64; 4]> = None;
    let mut add = |x: f64, y: f64| {
        let r = b.get_or_insert([x, y, x, y]);
        r[0] = r[0].min(x);
        r[1] = r[1].min(y);
        r[2] = r[2].max(x);
        r[3] = r[3].max(y);
    };
    for s in path {
        match *s {
            Seg::Move(x, y) | Seg::Line(x, y) => add(x, y),
            Seg::Curve(a, b2, c, d, e, f) => {
                add(a, b2);
                add(c, d);
                add(e, f);
            }
            Seg::Close => {}
        }
    }
    b
}

/// The most dashes a stroke may break into; beyond, it is drawn solid (a tiny pattern on a
/// long path would otherwise make billions of pieces).
pub const MAX_DASHES: f64 = 100_000.0;

/// Whether dash pattern `dash` on `path` (in the space the pattern is measured in) stays within
/// [`MAX_DASHES`]; its lengths must be finite and not all zero.
pub fn dashes_ok(path: &[Seg], dash: &[f64]) -> bool {
    let period: f64 = dash.iter().sum();
    if dash.is_empty() || !period.is_finite() || period <= 0.0 || dash.iter().any(|d| !d.is_finite() || *d < 0.0) {
        return false;
    }
    let mut len = 0.0;
    let mut cur = (0.0, 0.0);
    for s in path {
        match *s {
            Seg::Move(x, y) => cur = (x, y),
            Seg::Line(x, y) => {
                len += (x - cur.0).hypot(y - cur.1);
                cur = (x, y);
            }
            Seg::Curve(a, b, c, d, e, f) => {
                len += (a - cur.0).hypot(b - cur.1) + (c - a).hypot(d - b) + (e - c).hypot(f - d);
                cur = (e, f);
            }
            Seg::Close => {}
        }
    }
    len.is_finite() && len / period * dash.len() as f64 <= MAX_DASHES
}

/// A path with its curves replaced by line segments (`flattenpath`).
pub fn flatten(path: &[Seg], tol: f64) -> Vec<Seg> {
    let mut out = Vec::with_capacity(path.len());
    let mut cur = (0.0, 0.0);
    for s in path {
        match *s {
            Seg::Curve(x1, y1, x2, y2, x3, y3) => {
                let d = ((x1 - cur.0).hypot(y1 - cur.1) + (x2 - x1).hypot(y2 - y1) + (x3 - x2).hypot(y3 - y2)).max(0.0);
                let n = ((d / tol.max(0.05)).sqrt().ceil() as usize).clamp(1, 256);
                for i in 1..=n {
                    let t = i as f64 / n as f64;
                    let mt = 1.0 - t;
                    let x = mt * mt * mt * cur.0 + 3.0 * mt * mt * t * x1 + 3.0 * mt * t * t * x2 + t * t * t * x3;
                    let y = mt * mt * mt * cur.1 + 3.0 * mt * mt * t * y1 + 3.0 * mt * t * t * y2 + t * t * t * y3;
                    out.push(Seg::Line(x, y));
                }
                cur = (x3, y3);
            }
            Seg::Move(x, y) | Seg::Line(x, y) => {
                cur = (x, y);
                out.push(*s);
            }
            Seg::Close => out.push(*s),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrices_compose_like_postscript() {
        let t = M([1.0, 0.0, 0.0, 1.0, 10.0, 20.0]);
        let s = M([2.0, 0.0, 0.0, 3.0, 0.0, 0.0]);
        // `10 20 translate 2 3 scale`: scale applies first, then translate.
        let ctm = s.then(&t);
        assert_eq!(ctm.apply(1.0, 1.0), (12.0, 23.0));
        let inv = ctm.inv().unwrap();
        let (x, y) = inv.apply(12.0, 23.0);
        assert!((x - 1.0).abs() < 1e-12 && (y - 1.0).abs() < 1e-12);
        assert!(M([0.0; 6]).inv().is_none());
    }

    #[test]
    fn a_full_circle_is_four_quarter_curves() {
        let (start, curves) = arc_curves(0.0, 0.0, 10.0, 0.0, 360.0);
        assert!((start.0 - 10.0).abs() < 1e-9);
        assert_eq!(curves.len(), 4);
        let last = curves.last().unwrap();
        assert!((last[4] - 10.0).abs() < 1e-9 && last[5].abs() < 1e-9);
        assert!(arc_curves(0.0, 0.0, 1.0, 0.0, f64::INFINITY).1.is_empty());
    }
}
