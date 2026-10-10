//! Kenten (圏点): emphasis marks set beside each character of a run, over it in horizontal text
//! and to its right in vertical text (where the frame's turn carries "above"), or under it / to
//! its left. They don't change line breaks or leading.
//!
//! Placement follows W3C JLReq (emphasis dots) and JIS X 4051: each mark is centred in its own em
//! box, that box sits next to the character's em box (plus the kenten distance) and is centred on
//! the character, or starts with it when the alignment is Start (肩付き).

use designcraft_doc::CharProps;
use designcraft_fonts::{FaceRef, FontDb, ScopedFonts};
use designcraft_geom::{Rect, Shape as _};

use crate::{PlacedGlyph, RunStyle, upright_in_vertical};

/// A run's kenten, resolved for layout ([`RunStyle::kenten_mark`]).
#[derive(Clone, Debug, PartialEq)]
pub struct KentenMark {
    /// The mark: one character, or a short custom string.
    pub text: String,
    /// Family and style of a custom mark's font (`None` = the character's font).
    pub font: Option<(String, String)>,
    /// Size in points, and horizontal / vertical scale (1.0 = 100%).
    pub size: f64,
    pub x_scale: f64,
    pub y_scale: f64,
    /// Points between the character's em box and the mark's.
    pub distance: f64,
    /// Under the character (to its left in vertical text) rather than over it (to its right).
    pub below: bool,
    /// At the character's start rather than centred on it.
    pub start: bool,
    /// The marks' run style (their colour) in the composed story's style table.
    pub style: u32,
}

/// Largest mark size and distance, in points (the largest type size).
const MAX_POINTS: f64 = 1296.0;

fn finite_or(v: f64, default: f64) -> f64 {
    if v.is_finite() { v } else { default }
}

/// The mark size: the set size, else half the text size.
fn mark_size(p: &CharProps) -> f64 {
    p.kenten_size.filter(|v| v.is_finite() && *v > 0.0).unwrap_or(finite_or(p.size, 12.0) * 0.5).clamp(0.1, MAX_POINTS)
}

impl KentenMark {
    /// The kenten of text formatted with `p`, drawn in run style `style`.
    pub(crate) fn of(p: &CharProps, style: u32) -> KentenMark {
        use designcraft_doc::cjk_settings::{KentenAlignment, KentenKind, KentenPosition, kenten_mark};
        let custom_font = p.kenten_kind == KentenKind::Custom && !p.kenten_font.trim().is_empty();
        let font_style = if p.kenten_font_style.trim().is_empty() { "Regular".to_string() } else { p.kenten_font_style.clone() };
        KentenMark {
            text: kenten_mark(p.kenten_kind, &p.kenten_character),
            font: custom_font.then(|| (p.kenten_font.clone(), font_style)),
            size: mark_size(p),
            x_scale: finite_or(p.kenten_x_scale, 1.0).clamp(0.01, 10.0),
            y_scale: finite_or(p.kenten_y_scale, 1.0).clamp(0.01, 10.0),
            distance: finite_or(p.kenten_distance, 0.0).clamp(-MAX_POINTS, MAX_POINTS),
            below: p.kenten_position == KentenPosition::BelowLeft,
            start: p.kenten_alignment == KentenAlignment::Start,
            style,
        }
    }
}

/// The marks' own character format (interned as their run style): Kenten Color over the text's
/// colour, at the mark size, without the text's decorations.
pub(crate) fn mark_props(p: &CharProps) -> CharProps {
    let tint = |t: Option<f32>, text: f32| t.filter(|t| t.is_finite()).map_or(text, |t| t.clamp(0.0, 1.0));
    let mut m = p.clone();
    m.kenten = false;
    m.size = mark_size(p);
    if !p.kenten_fill.is_empty() {
        m.fill.clone_from(&p.kenten_fill);
    }
    m.fill_tint = tint(p.kenten_fill_tint, p.fill_tint);
    if !p.kenten_stroke.is_empty() {
        m.stroke.clone_from(&p.kenten_stroke);
    }
    m.stroke_tint = tint(p.kenten_stroke_tint, p.stroke_tint);
    if let Some(w) = p.kenten_stroke_weight.filter(|w| w.is_finite()) {
        m.stroke_weight = w.clamp(0.0, 1000.0);
    }
    m.underline = false;
    m.strikethrough = false;
    m.skew = 0.0;
    m.shatai_magnification = 0.0;
    m.ruby = String::new();
    m.warichu = false;
    m.xml_tag = String::new();
    m
}

/// Add the kenten of a laid-out line's characters. `glyph_fallback`: the document draws missing
/// glyphs from fallback fonts (a missing font's substitute always does).
pub(crate) fn annotate(db: &ScopedFonts<'_>, styles: &[RunStyle], line: &mut Vec<PlacedGlyph>, glyph_fallback: bool) {
    if !line.iter().any(|g| styles.get(g.style as usize).is_some_and(|s| s.kenten_mark.is_some())) {
        return;
    }
    let outlines = FontDb::global();
    let mut extra = Vec::new();
    for b in line.iter() {
        let Some(st) = styles.get(b.style as usize) else { continue };
        let Some(m) = &st.kenten_mark else { continue };
        // Characters only: not generated glyphs (ruby, labels), controls or spaces.
        if b.len == 0 || !b.visible || b.adv <= 0.0 || outlines.outline(&b.face, b.gid).elements().is_empty() {
            continue;
        }
        extra.extend(marks(db, b, m, glyph_fallback || st.missing_font));
    }
    line.extend(extra);
}

/// `text` shaped in `face` at the mark's size and scales, from x = 0 on the baseline.
fn shape_mark(face: FaceRef, text: &str, m: &KentenMark, b: &PlacedGlyph) -> Vec<PlacedGlyph> {
    let k = m.size / face.units_per_em().max(1.0);
    let (sx, sy) = (k * m.x_scale, k * m.y_scale);
    let mut x = 0.0;
    let mut out = Vec::new();
    for sg in designcraft_fonts::shape(&face, text, &[], |c| c) {
        let ch = text.get(sg.cluster..).and_then(|t| t.chars().next()).unwrap_or(' ');
        let adv = f64::from(sg.x_advance) * sx;
        out.push(PlacedGlyph {
            face,
            gid: sg.gid,
            x: x + f64::from(sg.x_offset) * sx,
            y: -f64::from(sg.y_offset) * sy,
            adv,
            sx,
            sy,
            style: m.style,
            byte: b.byte,
            len: 0,
            visible: true,
            upright: upright_in_vertical(ch),
            tcy: None,
            rtl: false,
            dx: 0.0,
        });
        x += adv;
    }
    out
}

/// The marks beside base character `b`. A mark the font lacks is its missing-glyph box, unless
/// `fallback`: then a fallback font draws it, or failing that a bullet.
fn marks(db: &ScopedFonts<'_>, b: &PlacedGlyph, m: &KentenMark, fallback: bool) -> Vec<PlacedGlyph> {
    let mut face = match &m.font {
        Some((family, style)) => FaceRef::of(&db.face(family, style)),
        None => b.face,
    };
    if fallback
        && let Some(c) = m.text.chars().next()
        && !face.covers(c)
        && let Some(f) = db.fallback_for(c, face.id(), None)
    {
        face = FaceRef::of(&f);
    }
    let mut glyphs = shape_mark(face, &m.text, m, b);
    if fallback && glyphs.first().is_some_and(|d| d.gid == 0) {
        glyphs = shape_mark(b.face, "\u{2022}", m, b);
    }
    // The character's em box across the line (y down from the line's baseline).
    let (em_top, em_bottom) = b.face.em_box();
    let (base_top, base_bottom) = (b.y - em_top * b.sy, b.y - em_bottom * b.sy);
    let (w, h) = (m.size * m.x_scale, m.size * m.y_scale);
    let (box_top, box_bottom) =
        if m.below { (base_bottom + m.distance, base_bottom + m.distance + h) } else { (base_top - m.distance - h, base_top - m.distance) };
    let centre_x = if m.start { b.x + w / 2.0 } else { b.x + b.adv / 2.0 };
    let centre_y = (box_top + box_bottom) / 2.0;
    // Centre the marks' ink in their em box (a mark's advance and baseline may hold it off centre).
    let outlines = FontDb::global();
    let ink = glyphs
        .iter()
        .map(|g| {
            let r = outlines.outline(&g.face, g.gid).bounding_box();
            Rect::new(g.x + r.x0 * g.sx, g.y + r.y0 * g.sy, g.x + r.x1 * g.sx, g.y + r.y1 * g.sy)
        })
        .filter(|r| r.width() > 0.0 && r.height() > 0.0)
        .reduce(|a, r| a.union(r));
    let (dx, dy) = match ink {
        Some(r) => (centre_x - (r.x0 + r.x1) / 2.0, centre_y - (r.y0 + r.y1) / 2.0),
        None => (centre_x - glyphs.iter().map(|g| g.adv).sum::<f64>() / 2.0, box_bottom),
    };
    for g in &mut glyphs {
        g.x += dx;
        g.y += dy;
    }
    glyphs
}
