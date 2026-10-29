//! Windows Metafile (WMF) drawings, replayed into SVG.
//!
//! InDesign places a WMF as a vector with a transparent background. This module reads the
//! placeable (or standard) header and the GDI records a clip-art WMF uses — pens, brushes,
//! polygons, lines, rectangles, ellipses and round rectangles — and writes them as SVG. The
//! screen rasterises that SVG with resvg and PDF export draws it as vectors, so the file stays
//! the document's source. Records that need GDI text, bitmaps, patterns or regions are refused
//! (the file reports "unsupported"), never drawn partially; damaged files return `None`.

use std::collections::BTreeSet;

/// A placeable WMF starts with this key (bytes `D7 CD C6 9A`).
const PLACEABLE_KEY: u32 = 0x9AC6_CDD7;

/// Bounds on hostile files.
const MAX_RECORDS: usize = 1_000_000;
const MAX_OBJECTS: usize = 16_384;
const MAX_SAVED: usize = 64;
const MAX_SVG: usize = 32 << 20;

/// The header of a WMF: where its records start, the drawing's bounds in metafile units and
/// those units per inch.
struct Header {
    records: usize,
    bbox: [f64; 4],
    inch: f64,
}

impl Header {
    fn parse(bytes: &[u8]) -> Option<Self> {
        let mut at = 0usize;
        let mut placeable_bbox = None;
        let mut inch = 96.0f64;
        if u32_at(bytes, 0)? == PLACEABLE_KEY {
            let l = f64::from(i16_at(bytes, 6)?);
            let t = f64::from(i16_at(bytes, 8)?);
            let r = f64::from(i16_at(bytes, 10)?);
            let b = f64::from(i16_at(bytes, 12)?);
            if r <= l || b <= t {
                return None;
            }
            let units = u16_at(bytes, 14)?;
            if units > 0 {
                inch = f64::from(units);
            }
            placeable_bbox = Some([l, t, r, b]);
            at = 22;
        }
        // The standard metafile header: type (memory or disk), size in words, version.
        let kind = u16_at(bytes, at)?;
        if kind != 1 && kind != 2 {
            return None;
        }
        let header_words = u16_at(bytes, at + 2)?;
        if !(9..=64).contains(&header_words) {
            return None;
        }
        let version = u16_at(bytes, at + 4)?;
        if !matches!(version, 0x0100 | 0x0300 | 0x0301) {
            return None;
        }
        let records = at.checked_add(usize::from(header_words).checked_mul(2)?)?;
        if records > bytes.len() {
            return None;
        }
        let bbox = match placeable_bbox {
            Some(b) => b,
            None => {
                // A standard WMF carries no physical size: use its drawing window (usually
                // written by the first records) and the usual 96 units per inch.
                let (w, h) = window_extent(bytes, records).unwrap_or((96.0, 96.0));
                [0.0, 0.0, w, h]
            }
        };
        if !(bbox[2] > bbox[0] && bbox[3] > bbox[1]) {
            return None;
        }
        Some(Self { records, bbox, inch })
    }

    fn width(&self) -> f64 {
        self.bbox[2] - self.bbox[0]
    }

    fn height(&self) -> f64 {
        self.bbox[3] - self.bbox[1]
    }

    fn size_points(&self) -> Option<(f64, f64)> {
        let w = self.width() * 72.0 / self.inch;
        let h = self.height() * 72.0 / self.inch;
        (w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0).then_some((w, h))
    }
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    let s = b.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([*s.first()?, *s.get(1)?]))
}

fn i16_at(b: &[u8], at: usize) -> Option<i16> {
    u16_at(b, at).map(|v| v as i16)
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?]))
}

/// A GDI COLORREF: 0x00BBGGRR.
fn colour_at(b: &[u8], at: usize) -> Option<[u8; 3]> {
    let c = u32_at(b, at)?;
    Some([(c & 0xFF) as u8, ((c >> 8) & 0xFF) as u8, ((c >> 16) & 0xFF) as u8])
}

/// The drawing window's x and y extent, from the first `SETWINDOWEXT` record, for a standard
/// WMF without a placeable header.
fn window_extent(bytes: &[u8], mut at: usize) -> Option<(f64, f64)> {
    for _ in 0..MAX_RECORDS {
        let words = u32_at(bytes, at)?;
        if words < 3 {
            return None;
        }
        let len = usize::try_from(words).ok()?.checked_mul(2)?;
        let end = at.checked_add(len)?;
        if end > bytes.len() {
            return None;
        }
        let function = u16_at(bytes, at + 4)?;
        if function == 0 {
            return None;
        }
        if function == 0x020C {
            // SETWINDOWEXT stores y then x.
            let y = f64::from(i16_at(bytes, at + 6)?.unsigned_abs());
            let x = f64::from(i16_at(bytes, at + 8)?.unsigned_abs());
            return (x > 0.0 && y > 0.0).then_some((x, y));
        }
        at = end;
    }
    None
}

/// Is this a WMF?
pub fn is_wmf(bytes: &[u8]) -> bool {
    Header::parse(bytes).is_some()
}

/// The natural size in points, from the placeable header's bounding box and units per inch.
/// Validates every record as well, so a WMF we can't draw is refused at placement time.
pub(crate) fn size(bytes: &[u8]) -> Option<(f64, f64)> {
    let header = Header::parse(bytes)?;
    draw(bytes, &header)?;
    header.size_points()
}

/// The drawing as an SVG document, or `None` when it has records we can't draw faithfully or is
/// damaged. The background is transparent: only the drawing records put paint down.
pub(crate) fn to_svg(bytes: &[u8]) -> Option<String> {
    let header = Header::parse(bytes)?;
    let body = draw(bytes, &header)?;
    let (width, height) = header.size_points()?;
    let mut svg = String::new();
    svg.try_reserve(body.len().checked_add(256)?).ok()?;
    svg.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"");
    svg.push_str(&num(width));
    svg.push_str("pt\" height=\"");
    svg.push_str(&num(height));
    svg.push_str("pt\" viewBox=\"0 0 ");
    svg.push_str(&num(header.width()));
    svg.push(' ');
    svg.push_str(&num(header.height()));
    svg.push_str("\">");
    svg.push_str(&body);
    svg.push_str("</svg>");
    Some(svg)
}

/// Replay the records, returning the SVG body (no `<svg>` wrapper).
fn draw(bytes: &[u8], header: &Header) -> Option<String> {
    let mut state = State::new(header);
    let mut out = String::new();
    let mut at = header.records;
    for _ in 0..MAX_RECORDS {
        let words = u32_at(bytes, at)?;
        // Every record has a 6-byte header; META_EOF is three words with no parameters.
        if words < 3 {
            return None;
        }
        let len = usize::try_from(words).ok()?.checked_mul(2)?;
        let end = at.checked_add(len)?;
        if end > bytes.len() {
            return None;
        }
        if u16_at(bytes, at + 4)? == 0 {
            return Some(out);
        }
        state.record(u16_at(bytes, at + 4)?, bytes.get(at + 6..end)?, &mut out)?;
        at = end;
    }
    None
}

/// The logical-to-SVG plane. GDI's window/viewport mapping is applied, then the drawing is
/// normalised to the placeable header's bounding box.
#[derive(Clone)]
struct Map {
    origin: (f64, f64),
    height: f64,
    win_org: (f64, f64),
    win_ext: (f64, f64),
    vp_org: (f64, f64),
    vp_ext: (f64, f64),
    mode: u16,
}

impl Map {
    fn new(header: &Header) -> Self {
        let (w, h) = (header.width(), header.height());
        Self {
            origin: (header.bbox[0], header.bbox[1]),
            height: h,
            win_org: (0.0, 0.0),
            win_ext: (w, h),
            // The viewport is the placeable bbox: its origin is the bbox's origin in device units.
            vp_org: (header.bbox[0], header.bbox[1]),
            vp_ext: (w, h),
            mode: 1,
        }
    }

    fn scale_x(&self) -> f64 {
        if self.win_ext.0.abs() > f64::EPSILON { self.vp_ext.0 / self.win_ext.0 } else { 1.0 }
    }

    fn scale_y(&self) -> f64 {
        if self.win_ext.1.abs() > f64::EPSILON { self.vp_ext.1 / self.win_ext.1 } else { 1.0 }
    }

    fn point(&self, x: f64, y: f64) -> (f64, f64) {
        let (mut sx, mut sy) = (self.scale_x(), self.scale_y());
        if self.mode == 7 {
            // MM_ISOTROPIC: one scale for both axes, keeping each axis' direction.
            let s = sx.abs().min(sy.abs());
            sx = s * sx.signum();
            sy = s * sy.signum();
        }
        let px = self.vp_org.0 + (x - self.win_org.0) * sx - self.origin.0;
        let mut py = self.vp_org.1 + (y - self.win_org.1) * sy - self.origin.1;
        if matches!(self.mode, 2..=6) {
            // The metric map modes have y growing upwards.
            py = self.height - py;
        }
        (px, py)
    }

    /// The pen width in SVG units: GDI logical units through the window mapping.
    fn pen_width(&self, width: f64) -> f64 {
        let width = if width > 0.0 { width } else { 1.0 };
        width * ((self.scale_x().abs() + self.scale_y().abs()) / 2.0).max(1e-6)
    }
}

#[derive(Clone)]
struct Brush {
    /// `None` for BS_NULL: the shape is not filled.
    colour: Option<[u8; 3]>,
}

#[derive(Clone)]
struct Pen {
    colour: [u8; 3],
    width: f64,
    style: u16,
}

#[derive(Clone)]
enum Object {
    Brush(Brush),
    Pen(Pen),
    Other,
}

#[derive(Clone)]
struct Saved {
    map: Map,
    polyfill: u16,
    brush: Brush,
    pen: Pen,
    current: Option<(f64, f64)>,
}

struct State {
    map: Map,
    polyfill: u16,
    objects: Vec<Option<Object>>,
    free: BTreeSet<usize>,
    brush: Brush,
    pen: Pen,
    current: Option<(f64, f64)>,
    saved: Vec<Saved>,
}

impl State {
    fn new(header: &Header) -> Self {
        Self {
            map: Map::new(header),
            polyfill: 1, // ALTERNATE, the GDI default
            objects: Vec::new(),
            free: BTreeSet::new(),
            // GDI's defaults: a solid white brush and a black cosmetic pen.
            brush: Brush { colour: Some([255, 255, 255]) },
            pen: Pen { colour: [0, 0, 0], width: 1.0, style: 0 },
            current: Some((0.0, 0.0)),
            saved: Vec::new(),
        }
    }

    fn record(&mut self, function: u16, p: &[u8], out: &mut String) -> Option<()> {
        match function {
            0x001E => self.save()?,
            0x0127 => self.restore(p)?,
            0x0103 => {
                let mode = u16_at(p, 0)?;
                if !(1..=8).contains(&mode) {
                    return None;
                }
                self.map.mode = mode;
            }
            0x020B => self.map.win_org = (f64::from(i16_at(p, 2)?), f64::from(i16_at(p, 0)?)),
            0x020C => self.set_extent(p, false),
            0x020D => self.map.vp_org = (f64::from(i16_at(p, 2)?), f64::from(i16_at(p, 0)?)),
            0x020E => self.set_extent(p, true),
            0x020F => {
                self.map.win_org.0 += f64::from(i16_at(p, 2)?);
                self.map.win_org.1 += f64::from(i16_at(p, 0)?);
            }
            0x0211 => {
                self.map.vp_org.0 += f64::from(i16_at(p, 2)?);
                self.map.vp_org.1 += f64::from(i16_at(p, 0)?);
            }
            0x0410 => self.scale_extent(p, false),
            0x0412 => self.scale_extent(p, true),
            0x0106 => {
                let mode = u16_at(p, 0)?;
                if !matches!(mode, 1 | 2) {
                    return None;
                }
                self.polyfill = mode;
            }
            0x0104 => {
                // R2_COPYPEN (13) is the default; another raster op would need compositing.
                if u16_at(p, 0)? != 13 {
                    return None;
                }
            }
            0x02FC => self.create(Object::Brush(parse_brush(p)?))?,
            0x02FA => self.create(Object::Pen(parse_pen(p)?))?,
            0x02FB | 0x00F7 | 0x06FF => self.create(Object::Other)?,
            0x012D => {
                let index = usize::from(u16_at(p, 0)?);
                if let Some(Some(object)) = self.objects.get(index) {
                    match object {
                        Object::Brush(b) => self.brush = b.clone(),
                        Object::Pen(pen) => self.pen = pen.clone(),
                        Object::Other => {}
                    }
                }
            }
            0x01F0 => self.delete(usize::from(u16_at(p, 0)?)),
            0x0324 => self.polygon(p, out)?,
            0x0325 => self.polyline(p, out)?,
            0x0538 => self.poly_polygon(p, out)?,
            0x0539 => self.poly_polyline(p, out)?,
            0x041B => self.rectangle(p, out)?,
            0x0418 => self.ellipse(p, out)?,
            0x061C => self.round_rect(p, out)?,
            0x0214 => {
                self.current = Some((f64::from(i16_at(p, 2)?), f64::from(i16_at(p, 0)?)));
            }
            0x0213 => self.line_to(p, out)?,
            0x041F => self.set_pixel(p, out)?,
            0x0626 => self.escape(p)?,
            // State that only affects text, bitmaps, palettes or fills we don't use here.
            0x0102 | 0x0105 | 0x0107 | 0x0108 | 0x0201 | 0x0209 | 0x020A | 0x012E | 0x0231 | 0x0035 | 0x0037 | 0x0139 | 0x0436 | 0x0149 => {}
            // Text, bitmaps, patterns, regions, clipping and the arc family: drawing them
            // partially would be worse than refusing the file.
            _ => return None,
        }
        Some(())
    }

    fn set_extent(&mut self, p: &[u8], viewport: bool) {
        let y = f64::from(i16_at(p, 0).unwrap_or(0));
        let x = f64::from(i16_at(p, 2).unwrap_or(0));
        if x == 0.0 || y == 0.0 {
            return;
        }
        if viewport {
            self.map.vp_ext = (x, y);
        } else {
            self.map.win_ext = (x, y);
        }
    }

    fn scale_extent(&mut self, p: &[u8], viewport: bool) {
        let Some(y) = i16_at(p, 0) else { return };
        let Some(x) = i16_at(p, 2) else { return };
        let (x, y) = (f64::from(x), f64::from(y));
        let ext = if viewport { &mut self.map.vp_ext } else { &mut self.map.win_ext };
        let (nx, ny) = (ext.0 * x, ext.1 * y);
        if nx.is_finite() && ny.is_finite() && nx != 0.0 && ny != 0.0 {
            *ext = (nx, ny);
        }
    }

    fn save(&mut self) -> Option<()> {
        if self.saved.len() >= MAX_SAVED {
            return None;
        }
        self.saved.push(Saved {
            map: self.map.clone(),
            polyfill: self.polyfill,
            brush: self.brush.clone(),
            pen: self.pen.clone(),
            current: self.current,
        });
        Some(())
    }

    fn restore(&mut self, p: &[u8]) -> Option<()> {
        let level = i16_at(p, 0)?;
        if level < 0 {
            for _ in 0..u32::from(level.unsigned_abs()) {
                let Some(saved) = self.saved.pop() else { break };
                self.apply(saved);
            }
        } else if level > 0 {
            // Positive: restore to that save level, discarding it and everything above it.
            let keep = usize::try_from(level).ok()?.checked_sub(1)?;
            while self.saved.len() > keep {
                let saved = self.saved.pop()?;
                self.apply(saved);
            }
        }
        Some(())
    }

    fn apply(&mut self, saved: Saved) {
        self.map = saved.map;
        self.polyfill = saved.polyfill;
        self.brush = saved.brush;
        self.pen = saved.pen;
        self.current = saved.current;
    }

    fn create(&mut self, object: Object) -> Option<()> {
        if let Some(slot) = self.free.pop_first() {
            if let Some(entry) = self.objects.get_mut(slot) {
                *entry = Some(object);
            }
        } else if self.objects.len() < MAX_OBJECTS {
            self.objects.push(Some(object));
        } else {
            return None;
        }
        Some(())
    }

    fn delete(&mut self, index: usize) {
        if let Some(entry) = self.objects.get_mut(index)
            && entry.take().is_some()
        {
            self.free.insert(index);
        }
    }

    fn points(&self, p: &[u8]) -> Option<Vec<(f64, f64)>> {
        let count = usize::from(u16_at(p, 0)?);
        if count < 2 {
            return None;
        }
        let need = 2usize.checked_add(count.checked_mul(4)?)?;
        if p.len() < need {
            return None;
        }
        let mut points = Vec::new();
        points.try_reserve_exact(count).ok()?;
        for i in 0..count {
            let x = f64::from(i16_at(p, 2 + i * 4)?);
            let y = f64::from(i16_at(p, 4 + i * 4)?);
            points.push(self.map.point(x, y));
        }
        Some(points)
    }

    fn polygon(&mut self, p: &[u8], out: &mut String) -> Option<()> {
        let points = self.points(p)?;
        self.emit_polygon(&points, out)
    }

    fn emit_polygon(&mut self, points: &[(f64, f64)], out: &mut String) -> Option<()> {
        let mut s = String::from("<polygon points=\"");
        points_attr(&mut s, points);
        s.push('"');
        s.push_str(&self.paint());
        s.push_str("/>");
        self.push(out, &s)
    }

    fn poly_polygon(&mut self, p: &[u8], out: &mut String) -> Option<()> {
        let count = usize::from(u16_at(p, 0)?);
        if count == 0 {
            return None;
        }
        let counts_at = 2usize;
        let points_at = counts_at.checked_add(count.checked_mul(2)?)?;
        let mut total = 0usize;
        for i in 0..count {
            total = total.checked_add(usize::from(u16_at(p, counts_at + i * 2)?))?;
        }
        if total < 2 || p.len() < points_at.checked_add(total.checked_mul(4)?)? {
            return None;
        }
        let mut at = points_at;
        for i in 0..count {
            let n = usize::from(u16_at(p, counts_at + i * 2)?);
            let mut points = Vec::new();
            points.try_reserve_exact(n).ok()?;
            for _ in 0..n {
                let x = f64::from(i16_at(p, at)?);
                let y = f64::from(i16_at(p, at + 2)?);
                points.push(self.map.point(x, y));
                at = at.checked_add(4)?;
            }
            if n >= 2 {
                self.emit_polygon(&points, out)?;
            }
        }
        Some(())
    }

    fn polyline(&mut self, p: &[u8], out: &mut String) -> Option<()> {
        let points = self.points(p)?;
        self.emit_polyline(&points, out)
    }

    fn poly_polyline(&mut self, p: &[u8], out: &mut String) -> Option<()> {
        let count = usize::from(u16_at(p, 0)?);
        if count == 0 {
            return None;
        }
        let counts_at = 2usize;
        let points_at = counts_at.checked_add(count.checked_mul(2)?)?;
        let mut total = 0usize;
        for i in 0..count {
            total = total.checked_add(usize::from(u16_at(p, counts_at + i * 2)?))?;
        }
        if total < 2 || p.len() < points_at.checked_add(total.checked_mul(4)?)? {
            return None;
        }
        let mut at = points_at;
        for i in 0..count {
            let n = usize::from(u16_at(p, counts_at + i * 2)?);
            let mut points = Vec::new();
            points.try_reserve_exact(n).ok()?;
            for _ in 0..n {
                points.push(self.map.point(f64::from(i16_at(p, at)?), f64::from(i16_at(p, at + 2)?)));
                at = at.checked_add(4)?;
            }
            if n >= 2 {
                self.emit_polyline(&points, out)?;
            }
        }
        Some(())
    }

    fn emit_polyline(&mut self, points: &[(f64, f64)], out: &mut String) -> Option<()> {
        if self.stroke_none() {
            return Some(());
        }
        let mut s = String::from("<polyline points=\"");
        points_attr(&mut s, points);
        s.push_str("\" fill=\"none\"");
        s.push_str(&self.stroke());
        s.push_str("/>");
        self.push(out, &s)
    }

    fn rectangle(&mut self, p: &[u8], out: &mut String) -> Option<()> {
        let (l, t, r, b) = Self::bounds(p)?;
        let (x0, y0) = self.map.point(l, t);
        let (x1, y1) = self.map.point(r, b);
        let (x, y) = (x0.min(x1), y0.min(y1));
        let (w, h) = ((x1 - x0).abs(), (y1 - y0).abs());
        if w <= 0.0 || h <= 0.0 {
            return Some(());
        }
        let s = format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/>", num(x), num(y), num(w), num(h), self.paint());
        self.push(out, &s)
    }

    fn ellipse(&mut self, p: &[u8], out: &mut String) -> Option<()> {
        let (l, t, r, b) = Self::bounds(p)?;
        let (x0, y0) = self.map.point(l, t);
        let (x1, y1) = self.map.point(r, b);
        let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let (rx, ry) = ((x1 - x0).abs() / 2.0, (y1 - y0).abs() / 2.0);
        if rx <= 0.0 || ry <= 0.0 {
            return Some(());
        }
        let s = format!("<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"{}/>", num(cx), num(cy), num(rx), num(ry), self.paint());
        self.push(out, &s)
    }

    fn round_rect(&mut self, p: &[u8], out: &mut String) -> Option<()> {
        // Height and width of the corner ellipse precede the bounds.
        let ch = f64::from(i16_at(p, 0)?.unsigned_abs()) * self.map.scale_y().abs() / 2.0;
        let cw = f64::from(i16_at(p, 2)?.unsigned_abs()) * self.map.scale_x().abs() / 2.0;
        let (l, t, r, b) = Self::bounds(p.get(4..)?)?;
        let (x0, y0) = self.map.point(l, t);
        let (x1, y1) = self.map.point(r, b);
        let (x, y) = (x0.min(x1), y0.min(y1));
        let (w, h) = ((x1 - x0).abs(), (y1 - y0).abs());
        if w <= 0.0 || h <= 0.0 {
            return Some(());
        }
        let s = format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" ry=\"{}\"{}/>",
            num(x),
            num(y),
            num(w),
            num(h),
            num(cw),
            num(ch),
            self.paint()
        );
        self.push(out, &s)
    }

    fn line_to(&mut self, p: &[u8], out: &mut String) -> Option<()> {
        let to = (f64::from(i16_at(p, 2)?), f64::from(i16_at(p, 0)?));
        let from = self.current.replace(to)?;
        if self.stroke_none() {
            return Some(());
        }
        let (x0, y0) = self.map.point(from.0, from.1);
        let (x1, y1) = self.map.point(to.0, to.1);
        let s = format!("<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"{}/>", num(x0), num(y0), num(x1), num(y1), self.stroke());
        self.push(out, &s)
    }

    fn set_pixel(&mut self, p: &[u8], out: &mut String) -> Option<()> {
        let colour = colour_at(p, 0)?;
        let (x, y) = self.map.point(f64::from(i16_at(p, 6)?), f64::from(i16_at(p, 4)?));
        let w = self.map.scale_x().abs().max(1e-6);
        let h = self.map.scale_y().abs().max(1e-6);
        let s = format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>", num(x), num(y), num(w), num(h), hex(colour));
        self.push(out, &s)
    }

    /// META_ESCAPE: printer/page control and queries have no effect on the drawing; drawing,
    /// clipping, embedded EMFs and pass-through data do, and are refused.
    fn escape(&mut self, p: &[u8]) -> Option<()> {
        match u16_at(p, 0)? {
            0x0001..=0x0008
            | 0x000A..=0x000E
            | 0x0010..=0x0012
            | 0x0014..=0x0018
            | 0x001A..=0x001C
            | 0x001D..=0x0020
            | 0x0022..=0x0023
            | 0x002A
            | 0x0100
            | 0x0102
            | 0x0201..=0x0202
            | 0x0801
            | 0x0C01
            | 0x1017..=0x1019 => Some(()),
            _ => None,
        }
    }

    fn stroke_none(&self) -> bool {
        self.pen.style & 0x0F == 5
    }

    fn stroke(&self) -> String {
        let width = self.map.pen_width(self.pen.width);
        let mut s = format!(" stroke=\"{}\" stroke-width=\"{}\"", hex(self.pen.colour), num(width));
        let dash = match self.pen.style & 0x0F {
            1 => format!(" stroke-dasharray=\"{} {}\"", num(3.0 * width), num(width)),
            2 => format!(" stroke-dasharray=\"{} {}\"", num(width), num(width)),
            3 => format!(" stroke-dasharray=\"{} {} {} {}\"", num(3.0 * width), num(width), num(width), num(width)),
            4 => format!(" stroke-dasharray=\"{} {} {} {} {} {}\"", num(3.0 * width), num(width), num(width), num(width), num(width), num(width)),
            _ => String::new(),
        };
        s.push_str(&dash);
        // The end-cap and join flags share the 16-bit pen style with the dash pattern.
        match self.pen.style & 0x0300 {
            0x0100 => s.push_str(" stroke-linecap=\"square\""),
            0x0200 => s.push_str(" stroke-linecap=\"butt\""),
            _ => {}
        }
        match self.pen.style & 0x3000 {
            0x1000 => s.push_str(" stroke-linejoin=\"bevel\""),
            0x2000 => s.push_str(" stroke-linejoin=\"miter\""),
            _ => {}
        }
        s
    }

    fn paint(&self) -> String {
        let fill = match self.brush.colour {
            Some(colour) => hex(colour),
            None => "none".to_string(),
        };
        let mut s = format!(" fill=\"{fill}\"");
        if self.stroke_none() {
            s.push_str(" stroke=\"none\"");
        } else {
            s.push_str(&self.stroke());
        }
        if self.polyfill == 1 && fill != "none" {
            s.push_str(" fill-rule=\"evenodd\"");
        }
        s
    }

    fn push(&self, out: &mut String, element: &str) -> Option<()> {
        if out.len().checked_add(element.len())? > MAX_SVG {
            return None;
        }
        out.push_str(element);
        Some(())
    }
    /// The four corners of a drawn rectangle in WMF order: bottom, right, top, left.
    fn bounds(p: &[u8]) -> Option<(f64, f64, f64, f64)> {
        let bottom = f64::from(i16_at(p, 0)?);
        let right = f64::from(i16_at(p, 2)?);
        let top = f64::from(i16_at(p, 4)?);
        let left = f64::from(i16_at(p, 6)?);
        Some((left, top, right, bottom))
    }
}

fn parse_brush(p: &[u8]) -> Option<Brush> {
    let style = u16_at(p, 0)?;
    let colour = colour_at(p, 2)?;
    match style {
        0 => Some(Brush { colour: Some(colour) }),
        1 => Some(Brush { colour: None }),
        // Hatched, patterned and bitmap brushes are a pattern we would silently flatten to a
        // solid colour; refuse the file instead.
        _ => None,
    }
}

fn parse_pen(p: &[u8]) -> Option<Pen> {
    let style = u16_at(p, 0)?;
    let dash = style & 0x0F;
    // Inside-frame, user-defined and alternate pens would change the geometry or dithering.
    if dash > 5 {
        return None;
    }
    let x = i16_at(p, 2)?.unsigned_abs();
    let y = i16_at(p, 4)?.unsigned_abs();
    let width = f64::from(x.max(y));
    let colour = colour_at(p, 6)?;
    Some(Pen { colour, width, style })
}

fn points_attr(out: &mut String, points: &[(f64, f64)]) {
    for (i, (x, y)) in points.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&num(*x));
        out.push(',');
        out.push_str(&num(*y));
    }
}

fn num(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    let mut s = format!("{v:.3}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    if s.is_empty() || s == "-0" { "0".to_string() } else { s }
}

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(function: u16, params: &[u8]) -> Vec<u8> {
        let words = (6 + params.len()).div_ceil(2);
        let mut r = Vec::new();
        r.extend(u32::try_from(words).unwrap().to_le_bytes());
        r.extend(function.to_le_bytes());
        r.extend(params);
        if params.len() % 2 == 1 {
            r.push(0);
        }
        r
    }

    fn eof() -> Vec<u8> {
        record(0, &[])
    }

    fn wmf(bbox: [i16; 4], inch: u16, records: &[Vec<u8>]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(PLACEABLE_KEY.to_le_bytes());
        b.extend(0u16.to_le_bytes());
        for v in bbox {
            b.extend(v.to_le_bytes());
        }
        b.extend(inch.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend(0u16.to_le_bytes());
        let body: Vec<u8> = records.concat();
        let words = (22 + 18 + body.len()) / 2;
        b.extend(1u16.to_le_bytes());
        b.extend(9u16.to_le_bytes());
        b.extend(0x0300u16.to_le_bytes());
        b.extend(u32::try_from(words).unwrap().to_le_bytes());
        b.extend(0u16.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend(0u16.to_le_bytes());
        b.extend(body);
        b
    }

    fn set_window_ext(x: i16, y: i16) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend(y.to_le_bytes());
        p.extend(x.to_le_bytes());
        record(0x020C, &p)
    }

    fn brush(colour: [u8; 3]) -> Vec<u8> {
        let reference = u32::from(colour[0]) | u32::from(colour[1]) << 8 | u32::from(colour[2]) << 16;
        let mut p = Vec::new();
        p.extend(0u16.to_le_bytes());
        p.extend(reference.to_le_bytes());
        p.extend(0u16.to_le_bytes());
        record(0x02FC, &p)
    }

    fn pen(colour: [u8; 3], width: i16) -> Vec<u8> {
        let reference = u32::from(colour[0]) | u32::from(colour[1]) << 8 | u32::from(colour[2]) << 16;
        let mut p = Vec::new();
        p.extend(0u16.to_le_bytes());
        p.extend(width.to_le_bytes());
        p.extend(width.to_le_bytes());
        p.extend(reference.to_le_bytes());
        p.extend(0u16.to_le_bytes());
        record(0x02FA, &p)
    }

    fn select(index: u16) -> Vec<u8> {
        record(0x012D, &index.to_le_bytes())
    }

    fn polygon(points: &[(i16, i16)]) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend(u16::try_from(points.len()).unwrap().to_le_bytes());
        for (x, y) in points {
            p.extend(x.to_le_bytes());
            p.extend(y.to_le_bytes());
        }
        record(0x0324, &p)
    }

    /// A rectangle-like record whose bounds are stored bottom, right, top, left.
    fn bounds_record(function: u16, left: i16, top: i16, right: i16, bottom: i16) -> Vec<u8> {
        let mut p = Vec::new();
        for v in [bottom, right, top, left] {
            p.extend(v.to_le_bytes());
        }
        record(function, &p)
    }

    #[test]
    fn rectangles_ellipses_and_round_rects_read_bottom_right_top_left_bounds() {
        let b = wmf([0, 0, 100, 100], 96, &[brush([255, 0, 0]), select(0), bounds_record(0x041B, 20, 10, 90, 80), eof()]);
        let svg = to_svg(&b).unwrap();
        assert!(svg.contains("<rect x=\"20\" y=\"10\" width=\"70\" height=\"70\""), "{svg}");

        let b = wmf([0, 0, 100, 100], 96, &[brush([255, 0, 0]), select(0), bounds_record(0x0418, 20, 10, 80, 90), eof()]);
        let svg = to_svg(&b).unwrap();
        assert!(svg.contains("<ellipse cx=\"50\" cy=\"50\" rx=\"30\" ry=\"40\""), "{svg}");

        // META_ROUNDRECT puts the corner ellipse's height and width before the bounds.
        let mut p = Vec::new();
        p.extend(20i16.to_le_bytes());
        p.extend(10i16.to_le_bytes());
        for v in [90i16, 80, 10, 20] {
            p.extend(v.to_le_bytes());
        }
        let b = wmf([0, 0, 100, 100], 96, &[brush([255, 0, 0]), select(0), record(0x061C, &p), eof()]);
        let svg = to_svg(&b).unwrap();
        assert!(svg.contains("<rect x=\"20\" y=\"10\" width=\"60\" height=\"80\" rx=\"5\" ry=\"10\""), "{svg}");
    }

    #[test]
    fn patterned_brushes_and_unsupported_pens_refuse_the_file() {
        // A hatched brush must not become a solid block of its foreground colour.
        let mut hatched = Vec::new();
        hatched.extend(2u16.to_le_bytes());
        hatched.extend(0x0000_00FFu32.to_le_bytes());
        hatched.extend(0u16.to_le_bytes());
        let b = wmf([0, 0, 100, 100], 96, &[record(0x02FC, &hatched), select(0), polygon(&[(10, 10), (90, 10), (50, 90)]), eof()]);
        assert_eq!(size(&b), None);
        assert_eq!(to_svg(&b), None);

        // PS_INSIDEFRAME changes the outline's geometry; it is refused, not drawn solid.
        let mut inside = Vec::new();
        inside.extend(6u16.to_le_bytes());
        inside.extend(1i16.to_le_bytes());
        inside.extend(1i16.to_le_bytes());
        inside.extend(0u32.to_le_bytes());
        inside.extend(0u16.to_le_bytes());
        let b = wmf([0, 0, 100, 100], 96, &[record(0x02FA, &inside), select(0), polygon(&[(10, 10), (90, 10), (50, 90)]), eof()]);
        assert_eq!(to_svg(&b), None);
    }

    #[test]
    fn pen_end_caps_and_joins_keep_their_flags() {
        let mut square = Vec::new();
        square.extend(0x0100u16.to_le_bytes()); // PS_SOLID | PS_ENDCAP_SQUARE
        square.extend(4i16.to_le_bytes());
        square.extend(4i16.to_le_bytes());
        square.extend(0u32.to_le_bytes());
        square.extend(0u16.to_le_bytes());
        let b = wmf([0, 0, 100, 100], 96, &[record(0x02FA, &square), select(0), polygon(&[(10, 10), (90, 10), (50, 90)]), eof()]);
        let svg = to_svg(&b).unwrap();
        assert!(svg.contains("stroke-linecap=\"square\""), "{svg}");
        assert!(!svg.contains("stroke-dasharray"), "the solid style is not a dash pattern: {svg}");
    }

    #[test]
    fn placeable_wmf_is_sized_from_its_header() {
        let b = wmf([0, 0, 402, 178], 96, &[set_window_ext(402, 178), eof()]);
        assert!(is_wmf(&b));
        assert_eq!(size(&b), Some((301.5, 133.5)));
        let svg = to_svg(&b).unwrap();
        assert!(svg.contains("viewBox=\"0 0 402 178\""), "{svg}");
        assert!(svg.contains("width=\"301.5pt\""), "{svg}");
        assert!(!svg.contains("<rect"), "no background is painted: {svg}");
    }

    #[test]
    fn a_polygon_uses_its_brush_and_pen() {
        let b = wmf(
            [0, 0, 100, 100],
            96,
            &[brush([255, 0, 0]), select(0), pen([0, 0, 0], 1), select(1), polygon(&[(10, 10), (90, 10), (90, 90), (10, 90)]), eof()],
        );
        let svg = to_svg(&b).unwrap();
        assert!(svg.contains("<polygon points=\"10,10 90,10 90,90 10,90\""), "{svg}");
        assert!(svg.contains("fill=\"#ff0000\""), "{svg}");
        assert!(svg.contains("fill-rule=\"evenodd\""), "GDI's default fill mode is alternate: {svg}");
        assert!(svg.contains("stroke=\"#000000\""), "{svg}");
        assert!(!svg.contains("<rect"), "the white default brush is not a background: {svg}");
    }

    #[test]
    fn object_slots_are_reused_lowest_first() {
        // Brush 0, pen 1; delete 0 and 1; the next two objects take 0 and 1 again.
        let b = wmf(
            [0, 0, 10, 10],
            96,
            &[
                brush([255, 0, 0]),
                pen([0, 0, 0], 1),
                record(0x01F0, &0u16.to_le_bytes()),
                record(0x01F0, &1u16.to_le_bytes()),
                brush([0, 0, 255]),
                select(0),
                polygon(&[(1, 1), (9, 1), (9, 9), (1, 9)]),
                eof(),
            ],
        );
        let svg = to_svg(&b).unwrap();
        assert!(svg.contains("fill=\"#0000ff\""), "{svg}");
    }

    #[test]
    fn unsupported_drawing_records_refuse_the_file() {
        // META_TEXTOUT: we can't set GDI text faithfully.
        let text = record(0x0521, &[0; 8]);
        let b = wmf([0, 0, 100, 100], 96, &[text, eof()]);
        assert!(is_wmf(&b));
        assert_eq!(size(&b), None);
        assert_eq!(to_svg(&b), None);
    }

    #[test]
    fn escapes_that_draw_or_clip_refuse_the_file() {
        let escape = |function: u16| {
            let mut p = Vec::new();
            p.extend(function.to_le_bytes());
            record(0x0626, &p)
        };
        // DRAWPATTERNRECT would paint a rectangle we can't reproduce.
        let b = wmf([0, 0, 100, 100], 96, &[escape(0x0019), eof()]);
        assert_eq!(size(&b), None);
        assert_eq!(to_svg(&b), None);
        // PASSTHROUGH hands drawing data to a printer driver.
        let b = wmf([0, 0, 100, 100], 96, &[escape(0x0013), eof()]);
        assert_eq!(to_svg(&b), None);
        // NEWFRAME is page control and leaves the drawing alone.
        let b = wmf([0, 0, 100, 100], 96, &[escape(0x0001), brush([255, 0, 0]), select(0), polygon(&[(10, 10), (90, 10), (50, 90)]), eof()]);
        assert!(to_svg(&b).is_some());
    }

    #[test]
    fn a_negative_window_extent_mirrors_like_gdi() {
        // Window origin at the bottom, y extent upward: y=10 maps to 90.
        let mut org = Vec::new();
        org.extend(100i16.to_le_bytes()); // y
        org.extend(0i16.to_le_bytes()); // x
        let b = wmf(
            [0, 0, 100, 100],
            96,
            &[record(0x020B, &org), set_window_ext(100, -100), brush([255, 0, 0]), select(0), polygon(&[(10, 10), (90, 10), (50, 50)]), eof()],
        );
        let svg = to_svg(&b).unwrap();
        assert!(svg.contains("points=\"10,90 90,90 50,50\""), "{svg}");
    }

    #[test]
    fn a_line_starts_at_the_gdi_origin_when_no_move_to_precedes_it() {
        let mut p = Vec::new();
        p.extend(25i16.to_le_bytes()); // y
        p.extend(40i16.to_le_bytes()); // x
        let b = wmf([0, 0, 100, 100], 96, &[record(0x0213, &p), eof()]);
        let svg = to_svg(&b).unwrap();
        assert!(svg.contains("<line x1=\"0\" y1=\"0\" x2=\"40\" y2=\"25\""), "{svg}");
    }

    #[test]
    fn damaged_or_hostile_files_are_refused() {
        let good = wmf([0, 0, 100, 100], 96, &[polygon(&[(1, 1), (9, 9)]), eof()]);
        assert!(is_wmf(&good));
        assert!(!is_wmf(&good[..20]));
        assert!(!is_wmf(b"\x01\x00"));
        let mut b = good.clone();
        b[0] = 0;
        assert!(!is_wmf(&b));
        let mut b = good.clone();
        b[22 + 4] = 0x99;
        assert!(!is_wmf(&b));
        // A record whose size runs past the file.
        let mut b = good.clone();
        let at = 22 + 18;
        b[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(to_svg(&b), None);
        // No META_EOF: not a complete metafile.
        let no_eof = wmf([0, 0, 100, 100], 96, &[polygon(&[(1, 1), (9, 9)])]);
        assert_eq!(to_svg(&no_eof), None);
    }

    #[test]
    fn a_viewport_and_window_scale_is_applied() {
        let b = wmf([0, 0, 200, 100], 96, &[set_window_ext(400, 200), polygon(&[(0, 0), (400, 0), (400, 200)]), eof()]);
        let svg = to_svg(&b).unwrap();
        assert!(svg.contains("points=\"0,0 200,0 200,100\""), "{svg}");
    }
}
