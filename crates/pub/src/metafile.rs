// Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0
//! WMF and EMF pictures (Escher metafile blips) as SVG, from the public [MS-ODRAW] metafile
//! header, [MS-WMF] and [MS-EMF]. Filled and outlined shapes, lines and paths are drawn; text,
//! embedded bitmaps, arcs and pattern fills are left out and reported.

use std::collections::HashMap;
use std::fmt::Write as _;

/// Uncompressed metafiles larger than this are refused.
const MAX_METAFILE: usize = 64 * 1024 * 1024;
/// Aggregate retained SVG/path bytes are bounded; reaching the limit is reported.
const MAX_SVG: usize = 32 * 1024 * 1024;
const MAX_RECORDS: usize = 2_000_000;
const MAX_SAVED: usize = 1024;
const MAX_OBJECTS: usize = 65_536;
const KAPPA: f64 = 0.552_284_75;

pub(crate) const BLIP_EMF: u16 = 0xF01A;
pub(crate) const BLIP_WMF: u16 = 0xF01B;

pub(crate) const TEXT_LEFT_OUT: &str = "Text inside WMF/EMF pictures was left out.";
pub(crate) const BITMAP_LEFT_OUT: &str = "Bitmaps inside WMF/EMF pictures were left out.";
pub(crate) const LIMIT_LEFT_OUT: &str = "WMF/EMF conversion reached its size or work limit; remaining drawing was omitted.";

pub(crate) const OTHER_LEFT_OUT: &str = "Arcs and pattern fills inside WMF/EMF pictures were left out.";

/// A metafile converted to SVG: the document, its integer width and height, and what was
/// left out.
#[derive(Debug)]
pub(crate) struct Svg {
    pub data: Vec<u8>,
    pub size: (u32, u32),
    pub warnings: Vec<&'static str>,
}

/// An Escher metafile blip's data after its UIDs (34-byte metafile header, then the possibly
/// deflate-compressed metafile) as SVG; `None` if it holds nothing drawable or is damaged.
pub(crate) fn to_svg(kind: u16, blip: &[u8]) -> Option<Svg> {
    let raw = metafile_bytes(blip)?;
    // Metafile header: cbSize, rcBounds, ptSize (EMU), cbSave, compression, filter.
    let bounds = [i32_at(blip, 4)?, i32_at(blip, 8)?, i32_at(blip, 12)?, i32_at(blip, 16)?];
    let size_emu = (i32_at(blip, 20)?, i32_at(blip, 24)?);
    match kind {
        BLIP_WMF => wmf(&raw, bounds, size_emu),
        BLIP_EMF => emf(&raw, size_emu),
        _ => None,
    }
}

pub(crate) fn to_svg_report(kind: u16, blip: &[u8]) -> std::result::Result<Option<Svg>, &'static str> {
    if blip.get(32) == Some(&0xFE)
        && let Some(body) = blip.get(34..)
        && let Some(save) = u32_at(blip, 28)
        && body.len().min(save as usize) > MAX_METAFILE
    {
        return Err("WMF/EMF uncompressed picture exceeds the byte limit; its frame was kept empty.");
    }
    Ok(to_svg(kind, blip))
}

fn metafile_bytes(blip: &[u8]) -> Option<Vec<u8>> {
    metafile_bytes_with_limit(blip, MAX_METAFILE)
}

fn metafile_bytes_with_limit(blip: &[u8], limit: usize) -> Option<Vec<u8>> {
    let save = usize::try_from(u32_at(blip, 28)?).ok()?;
    let compression = *blip.get(32)?;
    let body = blip.get(34..)?;
    let body = body.get(..save.min(body.len()))?;
    match compression {
        0x00 => miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(body, limit).ok(),
        0xFE if body.len() <= limit => Some(body.to_vec()),
        _ => None,
    }
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    let s = b.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([*s.first()?, *s.get(1)?]))
}
fn i16_at(b: &[u8], at: usize) -> Option<f64> {
    u16_at(b, at).map(|v| f64::from(v as i16))
}
fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?]))
}
fn i32_at(b: &[u8], at: usize) -> Option<i32> {
    u32_at(b, at).map(|v| v as i32)
}
fn f32_at(b: &[u8], at: usize) -> Option<f64> {
    u32_at(b, at).map(|v| f64::from(f32::from_bits(v))).filter(|v| v.is_finite())
}

/// A COLORREF (0x00BBGGRR). Palette-index colours are approximated with black.
fn colorref(v: u32) -> [u8; 3] {
    if v >> 24 == 0x01 {
        return [0, 0, 0];
    }
    [(v & 0xff) as u8, ((v >> 8) & 0xff) as u8, ((v >> 16) & 0xff) as u8]
}

#[derive(Clone, Copy, Debug)]
struct Pen {
    color: Option<[u8; 3]>,
    /// Logical width; 0 is a hairline.
    width: f64,
}

#[derive(Clone, Copy, Debug)]
enum Obj {
    Pen(Pen),
    Brush(Option<[u8; 3]>),
    Other,
}

/// Row-vector affine transform: x' = x·a + y·c + e, y' = x·b + y·d + f.
type Xf = [f64; 6];
const IDENTITY: Xf = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `a` applied first, then `b`.
fn then(a: Xf, b: Xf) -> Xf {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

#[derive(Clone, Copy, Debug)]
struct Dc {
    pen: Pen,
    brush: Option<[u8; 3]>,
    winding: bool,
    world: Xf,
    map_mode: u32,
    win_org: (f64, f64),
    win_ext: Option<(f64, f64)>,
    vp_org: (f64, f64),
    vp_ext: Option<(f64, f64)>,
}

impl Default for Dc {
    fn default() -> Self {
        Dc {
            pen: Pen { color: Some([0, 0, 0]), width: 0.0 },
            brush: Some([255, 255, 255]),
            winding: false,
            world: IDENTITY,
            map_mode: 1,
            win_org: (0.0, 0.0),
            win_ext: None,
            vp_org: (0.0, 0.0),
            vp_ext: None,
        }
    }
}

/// Drawing state shared by both formats. Points are turned into output coordinates as they
/// are drawn, so later mapping changes don't move earlier shapes.
struct Canvas {
    wmf: bool,
    dc: Dc,
    saved: Vec<Dc>,
    /// WMF: the output space is the window in effect at the first drawing (its origin and
    /// signed extent); EMF: the output space is the header's device bounds (origin).
    frame: Option<((f64, f64), (f64, f64))>,
    origin: (f64, f64),
    cur: (f64, f64),
    /// EMF path bracket being recorded, and the finished path.
    recording: Option<String>,
    path: Option<String>,
    body: String,
    hair: f64,
    warnings: Vec<&'static str>,
    full: bool,
    limit: usize,
}

impl Canvas {
    fn new(wmf: bool) -> Self {
        Canvas {
            wmf,
            dc: Dc::default(),
            saved: Vec::new(),
            frame: None,
            origin: (0.0, 0.0),
            cur: (0.0, 0.0),
            recording: None,
            path: None,
            body: String::new(),
            hair: 1.0,
            warnings: Vec::new(),
            full: false,
            limit: MAX_SVG,
        }
    }

    fn warn(&mut self, w: &'static str) {
        if !self.warnings.contains(&w) {
            self.warnings.push(w);
        }
    }

    /// Logical → output transform.
    fn xf(&mut self) -> Xf {
        let dc = self.dc;
        if self.wmf {
            let ext = dc.win_ext.unwrap_or((1.0, 1.0));
            let (_, fext) = *self.frame.get_or_insert((dc.win_org, ext));
            let sx = if ext.0 != 0.0 { fext.0.abs() / ext.0 } else { 1.0 };
            let sy = if ext.1 != 0.0 { fext.1.abs() / ext.1 } else { 1.0 };
            return [sx, 0.0, 0.0, sy, -dc.win_org.0 * sx, -dc.win_org.1 * sy];
        }
        let page = match (dc.map_mode, dc.win_ext, dc.vp_ext) {
            (7 | 8, Some(w), Some(v)) if w.0 != 0.0 && w.1 != 0.0 => {
                let (sx, sy) = (v.0 / w.0, v.1 / w.1);
                [sx, 0.0, 0.0, sy, dc.vp_org.0 - dc.win_org.0 * sx, dc.vp_org.1 - dc.win_org.1 * sy]
            }
            _ => [1.0, 0.0, 0.0, 1.0, dc.vp_org.0 - dc.win_org.0, dc.vp_org.1 - dc.win_org.1],
        };
        let to_out = [1.0, 0.0, 0.0, 1.0, -self.origin.0, -self.origin.1];
        then(then(dc.world, page), to_out)
    }

    fn pt(xf: &Xf, (x, y): (f64, f64)) -> (f64, f64) {
        (x * xf[0] + y * xf[2] + xf[4], x * xf[1] + y * xf[3] + xf[5])
    }

    /// Path data for a polyline/polygon through logical points.
    fn poly_d(&mut self, pts: &[(f64, f64)], close: bool) -> String {
        if !self.room(pts.len().saturating_mul(768)) {
            return String::new();
        }
        let xf = self.xf();
        let mut d = String::new();
        for (i, p) in pts.iter().enumerate() {
            let (x, y) = Self::pt(&xf, *p);
            d.push(if i == 0 { 'M' } else { 'L' });
            push_xy(&mut d, x, y);
        }
        if close && !d.is_empty() {
            d.push('Z');
        }
        d
    }

    /// Bézier segments through logical control points (groups of three after the start).
    fn bezier_d(&mut self, start: Option<(f64, f64)>, pts: &[(f64, f64)]) -> String {
        if !self.room(pts.len().saturating_add(1).saturating_mul(768)) {
            return String::new();
        }
        let xf = self.xf();
        let mut d = String::new();
        let rest = match start {
            Some(s) => {
                d.push('M');
                let (x, y) = Self::pt(&xf, s);
                push_xy(&mut d, x, y);
                pts
            }
            None => pts,
        };
        for c in rest.chunks_exact(3) {
            d.push('C');
            for p in c {
                let (x, y) = Self::pt(&xf, *p);
                push_xy(&mut d, x, y);
                d.push(' ');
            }
        }
        d
    }

    fn rect_d(&mut self, l: f64, t: f64, r: f64, b: f64) -> String {
        self.poly_d(&[(l, t), (r, t), (r, b), (l, b)], true)
    }

    fn ellipse_d(&mut self, l: f64, t: f64, r: f64, b: f64) -> String {
        let (cx, cy, rx, ry) = ((l + r) / 2.0, (t + b) / 2.0, (r - l) / 2.0, (b - t) / 2.0);
        let (kx, ky) = (rx * KAPPA, ry * KAPPA);
        let pts = [
            (cx + rx, cy + ky),
            (cx + kx, cy + ry),
            (cx, cy + ry),
            (cx - kx, cy + ry),
            (cx - rx, cy + ky),
            (cx - rx, cy),
            (cx - rx, cy - ky),
            (cx - kx, cy - ry),
            (cx, cy - ry),
            (cx + kx, cy - ry),
            (cx + rx, cy - ky),
            (cx + rx, cy),
        ];
        let mut d = self.bezier_d(Some((cx + rx, cy)), &pts);
        d.push('Z');
        d
    }

    fn round_rect_d(&mut self, l: f64, t: f64, r: f64, b: f64, w: f64, h: f64) -> String {
        let (rx, ry) = ((w / 2.0).abs().min((r - l).abs() / 2.0), (h / 2.0).abs().min((b - t).abs() / 2.0));
        if rx == 0.0 || ry == 0.0 {
            return self.rect_d(l, t, r, b);
        }
        let (l, r) = (l.min(r), l.max(r));
        let (t, b) = (t.min(b), t.max(b));
        let (kx, ky) = (rx * (1.0 - KAPPA), ry * (1.0 - KAPPA));
        let xf = self.xf();
        let mut d = String::new();
        let mut seg = |c: char, pts: &[(f64, f64)]| {
            d.push(c);
            for p in pts {
                let (x, y) = Self::pt(&xf, *p);
                push_xy(&mut d, x, y);
                d.push(' ');
            }
        };
        seg('M', &[(l + rx, t)]);
        seg('L', &[(r - rx, t)]);
        seg('C', &[(r - kx, t), (r, t + ky), (r, t + ry)]);
        seg('L', &[(r, b - ry)]);
        seg('C', &[(r, b - ky), (r - kx, b), (r - rx, b)]);
        seg('L', &[(l + rx, b)]);
        seg('C', &[(l + kx, b), (l, b - ky), (l, b - ry)]);
        seg('L', &[(l, t + ry)]);
        seg('C', &[(l, t + ky), (l + kx, t), (l + rx, t)]);
        d.push('Z');
        d
    }

    fn room(&mut self, extra: usize) -> bool {
        let retained =
            self.body.len().saturating_add(self.recording.as_ref().map_or(0, String::len)).saturating_add(self.path.as_ref().map_or(0, String::len));
        if self.full || retained.saturating_add(extra).saturating_add(512) > self.limit {
            self.full = true;
            self.warn(LIMIT_LEFT_OUT);
            return false;
        }
        true
    }

    fn record_path(&mut self, data: &str) {
        if self.room(data.len())
            && let Some(recording) = &mut self.recording
        {
            recording.push_str(data);
        }
    }

    /// Draw (or, inside an EMF path bracket, record) path data.
    fn draw(&mut self, d: &str, fill: bool, stroke: bool) {
        if self.recording.is_some() {
            self.record_path(d);
            return;
        }
        self.paint(d, fill, stroke);
    }

    fn paint(&mut self, d: &str, fill: bool, stroke: bool) {
        if d.is_empty() || self.full {
            return;
        }
        let fill = self.dc.brush.filter(|_| fill);
        let pen = self.dc.pen;
        let stroke = pen.color.filter(|_| stroke);
        if fill.is_none() && stroke.is_none() {
            return;
        }
        // Reserve an upper bound for attributes before appending any path bytes.
        if !self.room(d.len().saturating_add(512)) {
            return;
        }
        let _ = write!(self.body, "<path d=\"{d}\"");
        match fill {
            Some(c) => {
                let _ = write!(self.body, " fill=\"{}\"", hex(c));
                if !self.dc.winding {
                    self.body.push_str(" fill-rule=\"evenodd\"");
                }
            }
            None => self.body.push_str(" fill=\"none\""),
        }
        if let Some(c) = stroke {
            let xf = self.xf();
            let scale = (xf[0] * xf[3] - xf[1] * xf[2]).abs().sqrt();
            let w = if pen.width > 0.0 { pen.width * scale } else { self.hair };
            let _ = write!(self.body, " stroke=\"{}\" stroke-width=\"{}\" stroke-linejoin=\"round\" stroke-linecap=\"round\"", hex(c), num(w));
        }
        self.body.push_str("/>\n");
    }

    fn save(&mut self) {
        if self.saved.len() < MAX_SAVED {
            self.saved.push(self.dc);
        }
    }

    /// RestoreDC: negative = relative to the newest, positive = absolute (1-based).
    fn restore(&mut self, n: i64) {
        let keep = if n < 0 {
            self.saved.len().saturating_sub(usize::try_from(-n).unwrap_or(usize::MAX))
        } else {
            usize::try_from(n).unwrap_or(0).saturating_sub(1)
        };
        if keep < self.saved.len() {
            self.saved.truncate(keep + 1);
            if let Some(dc) = self.saved.pop() {
                self.dc = dc;
            }
        }
    }

    fn select(&mut self, obj: Obj) {
        match obj {
            Obj::Pen(p) => self.dc.pen = p,
            Obj::Brush(b) => self.dc.brush = b,
            Obj::Other => {}
        }
    }

    fn finish(self, vw: f64, vh: f64) -> Option<Svg> {
        if (self.body.is_empty() && !self.full) || !(vw.is_finite() && vh.is_finite()) || vw <= 0.0 || vh <= 0.0 {
            return None;
        }
        // Integer size with enough resolution for the aspect ratio.
        let k = if vw.max(vh) < 1000.0 { 1000.0 / vw.max(vh) } else { 1.0 };
        let (w, h) = ((vw * k).round().clamp(1.0, 1e6) as u32, (vh * k).round().clamp(1.0, 1e6) as u32);
        let mut s = String::with_capacity(self.body.len() + 200);
        let _ = writeln!(
            s,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {} {}\" preserveAspectRatio=\"none\">",
            num(vw),
            num(vh)
        );
        s.push_str(&self.body);
        s.push_str("</svg>\n");
        Some(Svg { data: s.into_bytes(), size: (w, h), warnings: self.warnings })
    }
}

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn num(v: f64) -> String {
    let v = if v.is_finite() { (v * 100.0).round() / 100.0 } else { 0.0 };
    let v = if v == 0.0 { 0.0 } else { v };
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn push_xy(d: &mut String, x: f64, y: f64) {
    let _ = write!(d, "{} {}", num(x), num(y));
}

/// Hairline width: half a point at the picture's natural size.
fn hairline(view: f64, size_emu: i32) -> f64 {
    let pt = f64::from(size_emu) / 12_700.0;
    if pt > 0.0 && view > 0.0 { view / pt * 0.5 } else { 1.0 }
}

// ---------------------------------------------------------------- WMF

fn wmf(raw: &[u8], bounds: [i32; 4], size_emu: (i32, i32)) -> Option<Svg> {
    let mut at = 0usize;
    let mut c = Canvas::new(true);
    // An Aldus placeable header (key 0x9AC6CDD7) may precede the standard header.
    if u32_at(raw, 0)? == 0x9AC6_CDD7 {
        c.dc.win_org = (i16_at(raw, 6)?, i16_at(raw, 8)?);
        c.dc.win_ext = Some((i16_at(raw, 10)? - c.dc.win_org.0, i16_at(raw, 12)? - c.dc.win_org.1));
        at = 22;
    } else if bounds[2] != bounds[0] && bounds[3] != bounds[1] {
        c.dc.win_org = (f64::from(bounds[0]), f64::from(bounds[1]));
        c.dc.win_ext = Some((f64::from(bounds[2]) - f64::from(bounds[0]), f64::from(bounds[3]) - f64::from(bounds[1])));
    }
    let header_words = usize::from(u16_at(raw, at.checked_add(2)?)?);
    let objects = usize::from(u16_at(raw, at.checked_add(10)?)?).min(MAX_OBJECTS);
    if !matches!(u16_at(raw, at)?, 1 | 2) || header_words < 9 {
        return None;
    }
    at = at.checked_add(header_words * 2)?;
    let mut table: Vec<Option<Obj>> = vec![None; objects];
    let mut n = 0usize;
    let mut first_draw = true;
    while let (Some(words), Some(func)) = (u32_at(raw, at), u16_at(raw, at + 4)) {
        let len = usize::try_from(words).ok()?.checked_mul(2)?;
        if len < 6 || func == 0x0000 {
            break;
        }
        let Some(rec) = raw.get(at..at.checked_add(len)?) else { break };
        let draws = matches!(func, 0x0213 | 0x0324 | 0x0325 | 0x0538 | 0x041B | 0x0418 | 0x061C);
        if draws && first_draw {
            first_draw = false;
            let view = c.dc.win_ext.unwrap_or((1.0, 1.0));
            c.hair = hairline(view.0.abs(), size_emu.0);
        }
        wmf_record(&mut c, &mut table, func, rec);
        at = at.checked_add(len)?;
        n += 1;
        if n > MAX_RECORDS {
            c.full = true;
            c.warn(LIMIT_LEFT_OUT);
        }
        if c.full {
            break;
        }
    }
    let ext = c.frame.map(|(_, ext)| ext).or_else(|| c.full.then_some(c.dc.win_ext.unwrap_or((1.0, 1.0))))?;
    c.finish(ext.0.abs(), ext.1.abs())
}

/// One WMF record: state, objects, drawing, or a warning for what is left out.
fn wmf_record(c: &mut Canvas, table: &mut Vec<Option<Obj>>, func: u16, rec: &[u8]) {
    match func {
        0x020B | 0x020C | 0x0106 | 0x001E | 0x0127 => wmf_state(c, func, rec),
        0x02FA | 0x02FC | 0x02FB | 0x00F7 | 0x06FF | 0x01F9 | 0x0142 | 0x012D | 0x01F0 => wmf_object(c, table, func, rec),
        0x0214 | 0x0213 | 0x0324 | 0x0325 | 0x0538 | 0x041B | 0x0418 | 0x061C => wmf_draw(c, func, rec),
        0x0521 | 0x0A32 => c.warn(TEXT_LEFT_OUT),
        0x0922 | 0x0B23 | 0x0940 | 0x0B41 | 0x0F43 | 0x0D33 => c.warn(BITMAP_LEFT_OUT),
        0x0817 | 0x081A | 0x0830 => c.warn(OTHER_LEFT_OUT),
        _ => {}
    }
}

fn wmf_state(c: &mut Canvas, func: u16, rec: &[u8]) {
    let p = |i: usize| i16_at(rec, 6 + 2 * i);
    match func {
        0x020B => {
            if let (Some(y), Some(x)) = (p(0), p(1)) {
                c.dc.win_org = (x, y);
            }
        }
        0x020C => {
            if let (Some(y), Some(x)) = (p(0), p(1)) {
                c.dc.win_ext = Some((x, y));
            }
        }
        0x0106 => c.dc.winding = p(0) == Some(2.0),
        0x001E => c.save(),
        0x0127 => c.restore(p(0).map_or(-1, |v| v as i64)),
        _ => {}
    }
}

fn wmf_object(c: &mut Canvas, table: &mut Vec<Option<Obj>>, func: u16, rec: &[u8]) {
    let p = |i: usize| i16_at(rec, 6 + 2 * i);
    match func {
        // Objects take the lowest free slot of the table.
        0x02FA | 0x02FC | 0x02FB | 0x00F7 | 0x06FF | 0x01F9 | 0x0142 => {
            let obj = match func {
                0x02FA => {
                    let style = u16_at(rec, 6).unwrap_or(0);
                    let width = p(1).unwrap_or(0.0).abs();
                    let color = u32_at(rec, 12).map(colorref).unwrap_or([0, 0, 0]);
                    Obj::Pen(Pen { color: (style & 0xff != 5).then_some(color), width })
                }
                0x02FC => {
                    let style = u16_at(rec, 6).unwrap_or(0);
                    let color = u32_at(rec, 8).map(colorref).unwrap_or([0, 0, 0]);
                    Obj::Brush((style != 1).then_some(color))
                }
                0x01F9 | 0x0142 => {
                    c.warn(OTHER_LEFT_OUT);
                    Obj::Brush(None)
                }
                _ => Obj::Other,
            };
            if let Some(slot) = table.iter_mut().find(|s| s.is_none()) {
                *slot = Some(obj);
            } else if table.len() < MAX_OBJECTS {
                table.push(Some(obj));
            }
        }
        0x012D => {
            if let Some(Some(obj)) = p(0).and_then(|i| table.get(i as u16 as usize)).copied() {
                c.select(obj);
            }
        }
        0x01F0 => {
            if let Some(slot) = p(0).and_then(|i| table.get_mut(i as u16 as usize)) {
                *slot = None;
            }
        }
        _ => {}
    }
}

fn wmf_draw(c: &mut Canvas, func: u16, rec: &[u8]) {
    let p = |i: usize| i16_at(rec, 6 + 2 * i);
    match func {
        0x0214 => {
            if let (Some(y), Some(x)) = (p(0), p(1)) {
                c.cur = (x, y);
            }
        }
        0x0213 => {
            if let (Some(y), Some(x)) = (p(0), p(1)) {
                let d = c.poly_d(&[c.cur, (x, y)], false);
                c.draw(&d, false, true);
                c.cur = (x, y);
            }
        }
        0x0324 | 0x0325 => {
            let count = p(0).map_or(0, |v| v as u16 as usize);
            let pts = wmf_points(rec, 8, count);
            let d = c.poly_d(&pts, func == 0x0324);
            c.draw(&d, func == 0x0324, true);
        }
        0x0538 => {
            let polys = p(0).map_or(0, |v| v as u16 as usize);
            let mut at_pt = 8 + 2 * polys;
            let mut d = String::new();
            for i in 0..polys {
                let count = p(1 + i).map_or(0, |v| v as u16 as usize);
                let pts = wmf_points(rec, at_pt, count);
                let part = c.poly_d(&pts, true);
                if !c.room(d.len().saturating_add(part.len())) {
                    return;
                }
                d.push_str(&part);
                at_pt = at_pt.saturating_add(4 * count);
            }
            c.draw(&d, true, true);
        }
        0x041B | 0x0418 => {
            if let (Some(b), Some(r), Some(t), Some(l)) = (p(0), p(1), p(2), p(3)) {
                let d = if func == 0x041B { c.rect_d(l, t, r, b) } else { c.ellipse_d(l, t, r, b) };
                c.draw(&d, true, true);
            }
        }
        0x061C => {
            if let (Some(h), Some(w), Some(b), Some(r), Some(t), Some(l)) = (p(0), p(1), p(2), p(3), p(4), p(5)) {
                let d = c.round_rect_d(l, t, r, b, w, h);
                c.draw(&d, true, true);
            }
        }
        _ => {}
    }
}

fn wmf_points(rec: &[u8], at: usize, count: usize) -> Vec<(f64, f64)> {
    let mut out = Vec::with_capacity(count.min(rec.len() / 4));
    for i in 0..count {
        let o = at.saturating_add(4 * i);
        let (Some(x), Some(y)) = (i16_at(rec, o), i16_at(rec, o.saturating_add(2))) else { break };
        out.push((x, y));
    }
    out
}

// ---------------------------------------------------------------- EMF

/// Stock objects (index with the high bit set).
fn stock(i: u32) -> Obj {
    match i & 0x7fff_ffff {
        0 => Obj::Brush(Some([255, 255, 255])),
        1 => Obj::Brush(Some([192, 192, 192])),
        2 => Obj::Brush(Some([128, 128, 128])),
        3 => Obj::Brush(Some([64, 64, 64])),
        4 => Obj::Brush(Some([0, 0, 0])),
        5 => Obj::Brush(None),
        6 => Obj::Pen(Pen { color: Some([255, 255, 255]), width: 0.0 }),
        7 => Obj::Pen(Pen { color: Some([0, 0, 0]), width: 0.0 }),
        8 => Obj::Pen(Pen { color: None, width: 0.0 }),
        _ => Obj::Other,
    }
}

fn emf(raw: &[u8], size_emu: (i32, i32)) -> Option<Svg> {
    if u32_at(raw, 0)? != 1 || u32_at(raw, 40)? != 0x464D_4520 {
        return None;
    }
    let bounds = [i32_at(raw, 8)?, i32_at(raw, 12)?, i32_at(raw, 16)?, i32_at(raw, 20)?].map(f64::from);
    let (vw, vh) = (bounds[2] - bounds[0] + 1.0, bounds[3] - bounds[1] + 1.0);
    let mut c = Canvas::new(false);
    c.origin = (bounds[0], bounds[1]);
    c.hair = hairline(vw, size_emu.0);
    let mut table: HashMap<u32, Obj> = HashMap::new();
    let mut at = 0usize;
    let mut n = 0usize;
    while let (Some(ty), Some(size)) = (u32_at(raw, at), u32_at(raw, at + 4)) {
        let len = usize::try_from(size).ok()?;
        if len < 8 || ty == 14 {
            break;
        }
        let Some(rec) = raw.get(at..at.checked_add(len)?) else { break };
        emf_record(&mut c, &mut table, ty, rec, len);
        // MOVETOEX inside a path bracket starts a new figure.
        if ty == 27 {
            let xf = c.xf();
            let (x, y) = Canvas::pt(&xf, c.cur);
            if c.recording.is_some() {
                let mut part = String::from("M");
                push_xy(&mut part, x, y);
                c.record_path(&part);
            }
        }
        at = at.checked_add(len)?;
        n += 1;
        if n > MAX_RECORDS {
            c.full = true;
            c.warn(LIMIT_LEFT_OUT);
        }
        if c.full {
            break;
        }
    }
    c.finish(vw, vh)
}

/// One EMF record: drawing, path brackets, state, objects, or a warning for what is left out.
fn emf_record(c: &mut Canvas, table: &mut HashMap<u32, Obj>, ty: u32, rec: &[u8], len: usize) {
    match ty {
        2..=6 | 85..=89 | 7 | 8 | 90 | 91 | 42..=44 | 54 => emf_draw(c, ty, rec, len),
        59 | 60 | 61 | 62..=64 | 68 => emf_path(c, ty),
        9 | 10 | 11 | 12 | 17 | 19 | 27 | 33 | 34 | 35 | 36 => emf_state(c, ty, rec),
        37 | 38 | 95 | 39 | 93 | 94 | 49 | 82 | 40 => emf_object(c, table, ty, rec),
        83 | 84 | 96 | 97 | 108 => c.warn(TEXT_LEFT_OUT),
        76..=81 | 114..=116 => c.warn(BITMAP_LEFT_OUT),
        41 | 45..=47 | 55 => c.warn(OTHER_LEFT_OUT),
        _ => {}
    }
}

fn emf_draw(c: &mut Canvas, ty: u32, rec: &[u8], len: usize) {
    let i = |o: usize| i32_at(rec, o).map(f64::from);
    let u = |o: usize| u32_at(rec, o);
    let pair = |o: usize| Some((i(o)?, i(o + 4)?));
    match ty {
        2..=6 | 85..=89 => {
            let small = ty >= 85;
            let count = u(24).map_or(0, |v| v as usize);
            if !c.room(count.saturating_mul(768)) {
                return;
            }
            let pts = emf_points(rec, 28, count, small);
            let kind = if small { ty - 83 } else { ty };
            let d = match kind {
                2 => c.bezier_d(pts.first().copied(), pts.get(1..).unwrap_or_default()),
                3 | 4 => c.poly_d(&pts, kind == 3),
                5 => c.bezier_d(None, &pts),
                _ => {
                    if !c.room(pts.len().saturating_mul(768)) {
                        return;
                    }
                    let mut d = String::new();
                    let xf = c.xf();
                    for p in &pts {
                        let (x, y) = Canvas::pt(&xf, *p);
                        d.push('L');
                        push_xy(&mut d, x, y);
                    }
                    d
                }
            };
            // POLYBEZIERTO / POLYLINETO continue from the current position.
            let d = if matches!(kind, 5 | 6) {
                let xf = c.xf();
                let (x, y) = Canvas::pt(&xf, c.cur);
                let mut m = String::from("M");
                push_xy(&mut m, x, y);
                if c.recording.is_some() { d } else { m + &d }
            } else {
                d
            };
            if let Some(last) = pts.last() {
                c.cur = *last;
            }
            c.draw(&d, kind == 3, true);
        }
        7 | 8 | 90 | 91 => {
            let small = ty >= 90;
            let polys = u(24).map_or(0, |v| v as usize).min(len / 4);
            let mut at_pt = 32usize.saturating_add(4 * polys);
            let mut d = String::new();
            for k in 0..polys {
                let count = u(32 + 4 * k).map_or(0, |v| v as usize);
                if !c.room(count.saturating_mul(768)) {
                    return;
                }
                let pts = emf_points(rec, at_pt, count, small);
                let part = c.poly_d(&pts, matches!(ty, 8 | 91));
                if !c.room(d.len().saturating_add(part.len())) {
                    return;
                }
                d.push_str(&part);
                at_pt = at_pt.saturating_add(count.saturating_mul(if small { 4 } else { 8 }));
            }
            c.draw(&d, matches!(ty, 8 | 91), true);
        }
        42..=44 => {
            if let (Some(l), Some(t), Some(r), Some(b)) = (i(8), i(12), i(16), i(20)) {
                let d = match ty {
                    42 => c.ellipse_d(l, t, r, b),
                    43 => c.rect_d(l, t, r, b),
                    _ => c.round_rect_d(l, t, r, b, i(24).unwrap_or(0.0), i(28).unwrap_or(0.0)),
                };
                c.draw(&d, true, true);
            }
        }
        54 => {
            if let Some(p) = pair(8) {
                if c.recording.is_some() {
                    let xf = c.xf();
                    let (x0, y0) = Canvas::pt(&xf, c.cur);
                    let (x, y) = Canvas::pt(&xf, p);
                    let mut part = String::new();
                    if c.recording.as_ref().is_some_and(String::is_empty) {
                        part.push('M');
                        push_xy(&mut part, x0, y0);
                    }
                    part.push('L');
                    push_xy(&mut part, x, y);
                    c.record_path(&part);
                } else {
                    let d = c.poly_d(&[c.cur, p], false);
                    c.draw(&d, false, true);
                }
                c.cur = p;
            }
        }
        _ => {}
    }
}

fn emf_path(c: &mut Canvas, ty: u32) {
    match ty {
        59 => {
            c.recording = Some(String::new());
            c.path = None;
        }
        60 => c.path = c.recording.take(),
        61 => {
            c.record_path("Z");
        }
        62..=64 => {
            if let Some(d) = c.path.take() {
                c.paint(&d, ty != 64, ty != 62);
            }
        }
        68 => {
            c.recording = None;
            c.path = None;
        }
        _ => {}
    }
}

fn emf_state(c: &mut Canvas, ty: u32, rec: &[u8]) {
    let i = |o: usize| i32_at(rec, o).map(f64::from);
    let u = |o: usize| u32_at(rec, o);
    let pair = |o: usize| Some((i(o)?, i(o + 4)?));
    match ty {
        9 => c.dc.win_ext = pair(8),
        10 => c.dc.win_org = pair(8).unwrap_or(c.dc.win_org),
        11 => c.dc.vp_ext = pair(8),
        12 => c.dc.vp_org = pair(8).unwrap_or(c.dc.vp_org),
        17 => c.dc.map_mode = u(8).unwrap_or(1),
        19 => c.dc.winding = u(8) == Some(2),
        27 => c.cur = pair(8).unwrap_or(c.cur),
        33 => c.save(),
        34 => c.restore(i(8).map_or(-1, |v| v as i64)),
        35 | 36 => {
            let m = [f32_at(rec, 8), f32_at(rec, 12), f32_at(rec, 16), f32_at(rec, 20), f32_at(rec, 24), f32_at(rec, 28)];
            if let [Some(a), Some(b), Some(cc), Some(d), Some(e), Some(f)] = m {
                let xf = [a, b, cc, d, e, f];
                c.dc.world = match (ty, u(32)) {
                    (35, _) | (36, Some(4)) => xf,
                    (36, Some(1)) => IDENTITY,
                    (36, Some(2)) => then(xf, c.dc.world),
                    (36, Some(3)) => then(c.dc.world, xf),
                    _ => c.dc.world,
                };
            }
        }
        _ => {}
    }
}

fn emf_object(c: &mut Canvas, table: &mut HashMap<u32, Obj>, ty: u32, rec: &[u8]) {
    let i = |o: usize| i32_at(rec, o).map(f64::from);
    let u = |o: usize| u32_at(rec, o);
    match ty {
        37 => {
            if let Some(ih) = u(8) {
                let obj = if ih & 0x8000_0000 != 0 { Some(stock(ih)) } else { table.get(&ih).copied() };
                if let Some(obj) = obj {
                    c.select(obj);
                }
            }
        }
        38 | 95 => {
            let (style, width, color) = if ty == 38 { (u(12), i(16), u(24)) } else { (u(28), u(32).map(f64::from), u(40)) };
            let pen = Pen { color: (style.unwrap_or(0) & 0xff != 5).then(|| colorref(color.unwrap_or(0))), width: width.unwrap_or(0.0).abs() };
            if let Some(ih) = u(8).filter(|_| table.len() < MAX_OBJECTS) {
                table.insert(ih, Obj::Pen(pen));
            }
        }
        39 => {
            let style = u(12).unwrap_or(0);
            if style > 2 {
                c.warn(OTHER_LEFT_OUT);
            }
            let brush = (style != 1 && style <= 2).then(|| colorref(u(16).unwrap_or(0)));
            if let Some(ih) = u(8).filter(|_| table.len() < MAX_OBJECTS) {
                table.insert(ih, Obj::Brush(brush));
            }
        }
        93 | 94 => {
            c.warn(OTHER_LEFT_OUT);
            if let Some(ih) = u(8).filter(|_| table.len() < MAX_OBJECTS) {
                table.insert(ih, Obj::Brush(None));
            }
        }
        49 | 82 => {
            if let Some(ih) = u(8).filter(|_| table.len() < MAX_OBJECTS) {
                table.insert(ih, Obj::Other);
            }
        }
        40 => {
            if let Some(ih) = u(8) {
                table.remove(&ih);
            }
        }
        _ => {}
    }
}

fn emf_points(rec: &[u8], at: usize, count: usize, small: bool) -> Vec<(f64, f64)> {
    let step: usize = if small { 4 } else { 8 };
    let mut out = Vec::with_capacity(count.min(rec.len() / step));
    for k in 0..count {
        let o = at.saturating_add(step.saturating_mul(k));
        let p = if small {
            i16_at(rec, o).zip(i16_at(rec, o.saturating_add(2)))
        } else {
            i32_at(rec, o).map(f64::from).zip(i32_at(rec, o.saturating_add(4)).map(f64::from))
        };
        let Some(p) = p else { break };
        out.push(p);
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn compressed_and_uncompressed_payloads_obey_the_same_limit() {
        let raw = vec![1; 9];
        let compressed = blip(&raw, [0; 4], (1, 1));
        assert!(metafile_bytes_with_limit(&compressed, 8).is_none());
        let mut plain = vec![0; 34];
        plain[28..32].copy_from_slice(&9u32.to_le_bytes());
        plain[32] = 0xfe;
        plain.extend(&raw);
        assert!(metafile_bytes_with_limit(&plain, 8).is_none());
        assert_eq!(metafile_bytes_with_limit(&plain, 9).unwrap(), raw);
    }

    #[test]
    fn path_and_output_limits_are_checked_before_append_and_reported() {
        let mut c = Canvas::new(false);
        c.limit = 1100;
        c.recording = Some(String::new());
        c.record_path(&"M".repeat(580));
        assert!(!c.full);
        c.record_path(&"L".repeat(9));
        assert!(c.full);
        assert_eq!(c.recording.as_ref().unwrap().len(), 580);
        let svg = c.finish(10.0, 10.0).unwrap();
        assert!(svg.warnings.contains(&LIMIT_LEFT_OUT));
        assert!(svg.data.len() <= 1100);

        let mut c = Canvas::new(false);
        c.limit = 1100;
        c.paint(&"M".repeat(100), true, true);
        assert!(c.full && c.body.is_empty(), "reject before appending an oversized path");
        assert!(c.finish(10.0, 10.0).unwrap().warnings.contains(&LIMIT_LEFT_OUT));
        let mut c = Canvas::new(false);
        c.limit = 1100;
        assert!(c.poly_d(&[(0.0, 0.0); 2], false).is_empty());
        assert!(c.full, "bound path construction before allocating its formatted points");
    }

    /// A metafile blip body (after the UIDs): header plus zlib-compressed metafile.
    pub(crate) fn blip(raw: &[u8], bounds: [i32; 4], size_emu: (i32, i32)) -> Vec<u8> {
        let packed = miniz_oxide::deflate::compress_to_vec_zlib(raw, 6);
        let mut b = Vec::new();
        b.extend((raw.len() as u32).to_le_bytes());
        for v in bounds {
            b.extend(v.to_le_bytes());
        }
        b.extend(size_emu.0.to_le_bytes());
        b.extend(size_emu.1.to_le_bytes());
        b.extend((packed.len() as u32).to_le_bytes());
        b.extend([0x00, 0xFE]);
        b.extend(packed);
        b
    }

    fn wmf_rec(func: u16, params: &[i16]) -> Vec<u8> {
        let mut r = Vec::new();
        r.extend((3 + params.len() as u32).to_le_bytes());
        r.extend(func.to_le_bytes());
        for p in params {
            r.extend(p.to_le_bytes());
        }
        r
    }

    /// A WMF: window 0,0 200×100; red brush, no pen; a triangle; a text record.
    pub(crate) fn sample_wmf() -> Vec<u8> {
        let mut body = Vec::new();
        body.extend(wmf_rec(0x020B, &[0, 0]));
        body.extend(wmf_rec(0x020C, &[100, 200]));
        body.extend(wmf_rec(0x02FC, &[0, 0x00ff, 0x0000, 0])); // solid brush, COLORREF 0x000000FF
        body.extend(wmf_rec(0x02FA, &[5, 0, 0, 0, 0])); // null pen
        body.extend(wmf_rec(0x012D, &[0]));
        body.extend(wmf_rec(0x012D, &[1]));
        body.extend(wmf_rec(0x0324, &[3, 0, 100, 100, 0, 200, 100]));
        body.extend(wmf_rec(0x0521, &[2, 0x6948, 0, 0]));
        body.extend(wmf_rec(0x0000, &[]));
        let mut w = Vec::new();
        for v in [1u16, 9, 0x0300] {
            w.extend(v.to_le_bytes());
        }
        w.extend(((18 + body.len()) as u32 / 2).to_le_bytes());
        w.extend(2u16.to_le_bytes());
        w.extend(7u32.to_le_bytes());
        w.extend(0u16.to_le_bytes());
        w.extend(body);
        w
    }

    fn emf_rec(ty: u32, data: &[u8]) -> Vec<u8> {
        let mut r = Vec::new();
        r.extend(ty.to_le_bytes());
        r.extend((8 + data.len() as u32).to_le_bytes());
        r.extend(data);
        r
    }

    fn le(vals: &[i32]) -> Vec<u8> {
        vals.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    /// An EMF: bounds 0,0–99,49; a blue filled rectangle with a 2-unit green pen, and a
    /// stroked path drawn with a world transform scaling by 0.5.
    pub(crate) fn sample_emf() -> Vec<u8> {
        let mut header = le(&[0, 0, 99, 49, 0, 0, 2646, 1323]);
        header.extend(0x464D_4520u32.to_le_bytes());
        header.extend(le(&[0x10000, 0, 0, 3, 0, 0, 0, 1024, 768, 320, 240]));
        let mut e = emf_rec(1, &header);
        e.extend(emf_rec(39, &le(&[1, 0, 0x00ff_0000, 0])));
        e.extend(emf_rec(38, &le(&[2, 0, 2, 0, 0x0000_ff00])));
        e.extend(emf_rec(37, &le(&[1])));
        e.extend(emf_rec(37, &le(&[2])));
        e.extend(emf_rec(43, &le(&[10, 10, 60, 40])));
        let mut xf = Vec::new();
        for v in [0.5f32, 0.0, 0.0, 0.5, 0.0, 0.0] {
            xf.extend(v.to_le_bytes());
        }
        xf.extend(4u32.to_le_bytes());
        e.extend(emf_rec(36, &xf));
        e.extend(emf_rec(59, &[]));
        e.extend(emf_rec(27, &le(&[100, 0])));
        e.extend(emf_rec(54, &le(&[180, 80])));
        e.extend(emf_rec(60, &[]));
        e.extend(emf_rec(64, &le(&[0, 0, 0, 0])));
        e.extend(emf_rec(81, &[0; 72]));
        e.extend(emf_rec(14, &le(&[0, 16, 20])));
        let total = e.len() as u32;
        e.get_mut(48..52).unwrap().copy_from_slice(&total.to_le_bytes());
        e
    }

    #[test]
    fn wmf_polygon_becomes_svg() {
        let svg = to_svg(BLIP_WMF, &blip(&sample_wmf(), [0, 0, 200, 100], (2_540_000, 1_270_000))).unwrap();
        let text = String::from_utf8(svg.data.clone()).unwrap();
        assert_eq!(svg.size, (1000, 500));
        assert!(text.contains("viewBox=\"0 0 200 100\""), "{text}");
        assert!(text.contains("<path d=\"M0 100L100 0L200 100Z\" fill=\"#ff0000\" fill-rule=\"evenodd\"/>"), "{text}");
        assert_eq!(svg.warnings, vec![TEXT_LEFT_OUT]);
        assert!(designcraft_images::svg_tree(&svg.data).is_some());
    }

    #[test]
    fn wmf_flipped_window_maps_into_view() {
        // Window origin (0,100), extent (200,-100): y grows upwards.
        let mut raw = sample_wmf();
        let at = 18 + 6; // first record's params: SETWINDOWORG (y, x)
        raw.get_mut(at..at + 2).unwrap().copy_from_slice(&100i16.to_le_bytes());
        let at = 18 + 10 + 6; // SETWINDOWEXT (y, x)
        raw.get_mut(at..at + 2).unwrap().copy_from_slice(&(-100i16).to_le_bytes());
        let text = String::from_utf8(to_svg(BLIP_WMF, &blip(&raw, [0; 4], (0, 0))).unwrap().data).unwrap();
        assert!(text.contains("d=\"M0 0L100 100L200 0Z\""), "{text}");
    }

    #[test]
    fn emf_shapes_paths_and_world_transform() {
        let svg = to_svg(BLIP_EMF, &blip(&sample_emf(), [0; 4], (952_500, 476_250))).unwrap();
        let text = String::from_utf8(svg.data.clone()).unwrap();
        assert!(text.contains("viewBox=\"0 0 100 50\""), "{text}");
        assert!(
            text.contains("d=\"M10 10L60 10L60 40L10 40Z\" fill=\"#0000ff\" fill-rule=\"evenodd\" stroke=\"#00ff00\" stroke-width=\"2\""),
            "{text}"
        );
        assert!(text.contains("d=\"M50 0L90 40\" fill=\"none\" stroke=\"#00ff00\" stroke-width=\"1\""), "{text}");
        assert_eq!(svg.warnings, vec![BITMAP_LEFT_OUT]);
        assert!(designcraft_images::svg_tree(&svg.data).is_some());
    }

    #[test]
    fn damaged_metafiles_never_panic() {
        for (kind, raw) in [(BLIP_WMF, sample_wmf()), (BLIP_EMF, sample_emf())] {
            for cut in 0..raw.len() {
                let _ = to_svg(kind, &blip(raw.get(..cut).unwrap(), [0, 0, 10, 10], (1, 1)));
            }
            for i in 0..raw.len() {
                let mut bad = raw.clone();
                bad[i] ^= 0xA5;
                let _ = to_svg(kind, &blip(&bad, [0, 0, 10, 10], (1, 1)));
            }
        }
        assert!(to_svg(BLIP_WMF, &[0; 10]).is_none());
        assert!(to_svg(0xF01C, &blip(&sample_wmf(), [0; 4], (1, 1))).is_none());
    }
}
