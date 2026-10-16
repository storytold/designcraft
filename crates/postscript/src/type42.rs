//! Type 42 fonts: a TrueType font in the `sfnts` strings, glyphs found through CharStrings
//! (glyph name → glyph index), drawn in em units (Adobe Technical Note 5012).

use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::TableProvider;
use skrifa::{GlyphId, MetadataProvider};

use crate::gstate::Seg;
use crate::obj::{Key, Obj, PDict, name};
use crate::type1::Glyph;

/// The TrueType data of a Type 42 font: its `sfnts` strings joined (an odd string's last byte
/// is padding).
pub fn sfnts(font: &PDict) -> Option<Vec<u8>> {
    let Some(Obj::Array(a, _)) = font.get_name(b"sfnts") else { return None };
    let mut out = Vec::new();
    for s in a.items() {
        let Obj::Str(s, _) = s else { return None };
        let mut b = s.bytes();
        if b.len() % 2 == 1 {
            b.pop();
        }
        out.extend_from_slice(&b);
    }
    Some(out)
}

struct Pen {
    path: Vec<Seg>,
    scale: f64,
    cur: (f64, f64),
}

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = (f64::from(x) * self.scale, f64::from(y) * self.scale);
        self.path.push(Seg::Move(p.0, p.1));
        self.cur = p;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = (f64::from(x) * self.scale, f64::from(y) * self.scale);
        self.path.push(Seg::Line(p.0, p.1));
        self.cur = p;
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        // A quadratic as the cubic with the same shape.
        let (c, p) = ((f64::from(cx) * self.scale, f64::from(cy) * self.scale), (f64::from(x) * self.scale, f64::from(y) * self.scale));
        let a = (self.cur.0 + 2.0 / 3.0 * (c.0 - self.cur.0), self.cur.1 + 2.0 / 3.0 * (c.1 - self.cur.1));
        let b = (p.0 + 2.0 / 3.0 * (c.0 - p.0), p.1 + 2.0 / 3.0 * (c.1 - p.1));
        self.path.push(Seg::Curve(a.0, a.1, b.0, b.1, p.0, p.1));
        self.cur = p;
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let s = self.scale;
        let p = (f64::from(x) * s, f64::from(y) * s);
        self.path.push(Seg::Curve(f64::from(x1) * s, f64::from(y1) * s, f64::from(x2) * s, f64::from(y2) * s, p.0, p.1));
        self.cur = p;
    }
    fn close(&mut self) {
        self.path.push(Seg::Close);
    }
}

/// The glyph index for a glyph name (CharStrings maps names to indices).
pub fn glyph_index(font: &PDict, glyph: &[u8]) -> u32 {
    let Some(Obj::Dict(cs)) = font.get_name(b"CharStrings") else { return 0 };
    cs.get(&Key::Name(name(glyph))).and_then(|o| o.num()).and_then(|v| u32::try_from(v as i64).ok()).unwrap_or(0)
}

/// Glyph `gid` of TrueType `data` in em units, with its advance.
pub fn glyph(data: &[u8], gid: u32) -> Option<Glyph> {
    let f = skrifa::FontRef::new(data).ok()?;
    let upem = f64::from(f.head().ok()?.units_per_em()).max(1.0);
    let mut pen = Pen { path: Vec::new(), scale: 1.0 / upem, cur: (0.0, 0.0) };
    if let Some(g) = f.outline_glyphs().get(GlyphId::new(gid)) {
        let _ = g.draw(DrawSettings::unhinted(Size::unscaled(), LocationRef::default()), &mut pen);
    }
    let adv = f.glyph_metrics(Size::unscaled(), LocationRef::default()).advance_width(GlyphId::new(gid)).map_or(0.0, f64::from) / upem;
    Some(Glyph { cost: pen.path.len() * 4 + 16, path: pen.path, width: (adv, 0.0) })
}
