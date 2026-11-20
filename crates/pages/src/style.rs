//! Styles: paragraph, character and graphic styles, each a set of overrides on its parent
//! (`TSS.StyleArchive`: name in 1, parent reference in 3). Resolving one walks the chain from the
//! root down so a child's values win.

use std::collections::HashSet;

use crate::package::Package;
use crate::proto::Msg;

pub(crate) const CHARACTER_STYLE: u32 = 2021;
pub(crate) const PARAGRAPH_STYLE: u32 = 2022;
pub(crate) const GRAPHIC_STYLE: u32 = 2025;
pub(crate) const MEDIA_STYLE: u32 = 3016;

/// Deepest style chain followed.
const MAX_CHAIN: usize = 32;

/// sRGB red, green, blue and alpha, 0–1.
pub(crate) type Rgba = [f64; 4];

/// A `TSP.Color` (red, green and blue in 3–5, alpha in 6).
pub(crate) fn color(m: Msg) -> Option<Rgba> {
    let c = |n| m.f32(n).map(|v| v.clamp(0.0, 1.0));
    Some([c(3)?, c(4)?, c(5)?, c(6).unwrap_or(1.0)])
}

/// The style messages from the root of `id`'s chain down to `id`. `base` finds the
/// `TSS.StyleArchive` inside a style of this kind.
fn chain<'p>(pkg: &'p Package, id: u64, ty: u32, base: impl Fn(Msg<'p>) -> Option<Msg<'p>>) -> Vec<Msg<'p>> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut next = Some(id);
    while let Some(id) = next {
        if out.len() >= MAX_CHAIN || !seen.insert(id) {
            break;
        }
        let Some(m) = pkg.typed(id, ty) else { break };
        out.push(m);
        next = base(m).and_then(|b| b.reference(3));
    }
    out.reverse();
    out
}

macro_rules! overlay {
    ($self:ident, $o:ident: $($f:ident),*) => { $( if $o.$f.is_some() { $self.$f = $o.$f.clone(); } )* };
}

/// `TSWP.CharacterStylePropertiesArchive`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CharProps {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub size: Option<f64>,
    pub font: Option<String>,
    pub color: Option<Rgba>,
    pub baseline: Option<u64>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    pub caps: Option<u64>,
    pub shift: Option<f64>,
    pub tracking: Option<f64>,
}

impl CharProps {
    pub fn read(m: Msg) -> Self {
        CharProps {
            bold: m.bool(1),
            italic: m.bool(2),
            size: m.f32(3).filter(|v| *v > 0.0),
            font: m.str(5).filter(|s| !s.is_empty()).map(str::to_string),
            // The text colour, or the text fill (46) when it is a plain colour.
            color: m.msg(7).and_then(color).or_else(|| m.msg(46).and_then(|f| f.msg(1)).and_then(color)),
            baseline: m.uint(10),
            underline: m.uint(11).map(|v| v != 0),
            strike: m.uint(12).map(|v| v != 0),
            caps: m.uint(13),
            shift: m.f32(14),
            tracking: m.f32(27),
        }
    }

    pub fn overlay(&mut self, o: &CharProps) {
        overlay!(self, o: bold, italic, size, font, color, baseline, underline, strike, caps, shift, tracking);
    }
}

/// `TSWP.ParagraphStylePropertiesArchive`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ParaProps {
    pub align: Option<u64>,
    pub first_indent: Option<f64>,
    pub left_indent: Option<f64>,
    pub right_indent: Option<f64>,
    /// Line spacing: mode (none: relative, 1 at least, 2 exactly, 4 between) and amount.
    pub spacing: Option<(Option<u64>, f64)>,
    pub space_after: Option<f64>,
    pub space_before: Option<f64>,
}

impl ParaProps {
    fn read(m: Msg) -> Self {
        ParaProps {
            align: m.uint(1),
            first_indent: m.f32(7),
            left_indent: m.f32(11),
            right_indent: m.f32(19),
            spacing: m.msg(13).and_then(|s| Some((s.uint(1), s.f32(2)?))),
            space_after: m.f32(20),
            space_before: m.f32(21),
        }
    }

    fn overlay(&mut self, o: &ParaProps) {
        overlay!(self, o: align, first_indent, left_indent, right_indent, spacing, space_after, space_before);
    }
}

/// A paragraph style resolved: its character and paragraph properties.
pub(crate) fn paragraph_style(pkg: &Package, id: u64) -> (CharProps, ParaProps) {
    let mut c = CharProps::default();
    let mut p = ParaProps::default();
    for m in chain(pkg, id, PARAGRAPH_STYLE, |m| m.msg(1)) {
        if let Some(cm) = m.msg(11) {
            c.overlay(&CharProps::read(cm));
        }
        if let Some(pm) = m.msg(12) {
            p.overlay(&ParaProps::read(pm));
        }
    }
    (c, p)
}

/// A character style resolved.
pub(crate) fn character_style(pkg: &Package, id: u64) -> CharProps {
    let mut c = CharProps::default();
    for m in chain(pkg, id, CHARACTER_STYLE, |m| m.msg(1)) {
        if let Some(cm) = m.msg(11) {
            c.overlay(&CharProps::read(cm));
        }
    }
    c
}

/// A stroke: colour and width; `None` when the stroke is turned off.
pub(crate) type StrokeProps = Option<(Rgba, f64)>;

/// A graphic style resolved: what drawables and text boxes take from it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ShapeProps {
    /// `Some(None)`: no fill (or one this importer can't draw yet).
    pub fill: Option<Option<Rgba>>,
    pub gradient_or_image_fill: bool,
    pub stroke: Option<StrokeProps>,
    pub opacity: Option<f64>,
    pub shadow: bool,
    /// Text box vertical alignment: 0 top, 1 middle, 2 bottom.
    pub valign: Option<u64>,
    /// Text box insets: left, top, right, bottom.
    pub padding: Option<[f64; 4]>,
}

fn stroke(m: Msg) -> StrokeProps {
    // Pattern type 2 is "no stroke".
    if m.msg(6).and_then(|p| p.uint(1)) == Some(2) {
        return None;
    }
    let width = m.f32(2).unwrap_or(1.0);
    if width <= 0.0 {
        return None;
    }
    Some((m.msg(1).and_then(color).unwrap_or([0.0, 0.0, 0.0, 1.0]), width))
}

pub(crate) fn graphic_style(pkg: &Package, id: u64) -> ShapeProps {
    let mut s = ShapeProps::default();
    for m in chain(pkg, id, GRAPHIC_STYLE, |m| m.msg(1)?.msg(1)) {
        if let Some(p) = m.msg(1).and_then(|b| b.msg(11)) {
            if let Some(f) = p.msg(1) {
                let c = f.msg(1).and_then(color);
                s.gradient_or_image_fill = c.is_none() && (f.has(2) || f.has(3));
                // A gradient shows its first stop's colour until gradients are read.
                let first_stop = || f.msg(2)?.msgs(2).next()?.msg(1).and_then(color);
                s.fill = Some(c.or_else(first_stop));
            }
            if let Some(st) = p.msg(2) {
                s.stroke = Some(stroke(st));
            }
            if let Some(o) = p.f32(3) {
                s.opacity = Some(o.clamp(0.0, 1.0));
            }
            if let Some(sh) = p.msg(4) {
                s.shadow = sh.bool(6).unwrap_or(true);
            }
        }
        if let Some(layout) = m.msg(11) {
            s.valign = layout.uint(2).or(s.valign);
            if let Some(pad) = layout.msg(6) {
                let old = s.padding.unwrap_or([0.0; 4]);
                let v = |n: u32, i: usize| pad.f32(n).unwrap_or(old[i]);
                s.padding = Some([v(1, 0), v(2, 1), v(3, 2), v(4, 3)]);
            }
        }
    }
    s
}

/// An image's opacity from its media style (properties in 11, opacity in 11.2).
pub(crate) fn media_opacity(pkg: &Package, id: u64) -> Option<f64> {
    chain(pkg, id, MEDIA_STYLE, |m| m.msg(1)).iter().rev().find_map(|m| m.msg(11)?.f32(2)).map(|o| o.clamp(0.0, 1.0))
}
