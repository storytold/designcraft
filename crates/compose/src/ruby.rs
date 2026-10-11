//! Ruby and kenten: small glyphs set over laid-out text (to its right in vertical frames, where
//! the frame's turn carries "above" there). They don't change line breaks or leading.

use designcraft_fonts::ScopedFonts;
use designcraft_geom::Shape as _;

use crate::{PlacedGlyph, RunStyle, upright_in_vertical};

/// Ruby and emphasis marks are half the base size.
const SCALE: f64 = 0.5;

/// A glyph of `text` set at `size` in `base`'s font from `x`, above the base text; returns them
/// and their total advance.
fn set(base: &PlacedGlyph, text: &str, size: f64, base_size: f64) -> (Vec<PlacedGlyph>, f64) {
    let face = &base.face;
    let k = size / face.units_per_em();
    // Its baseline clears the base text's em box top.
    let y = -(base_size * 0.88 + size * 0.2);
    let mut x = 0.0;
    let mut out = Vec::new();
    for sg in designcraft_fonts::shape(face, text, &[], |c| c) {
        let ch = text[sg.cluster..].chars().next().unwrap_or(' ');
        let adv = sg.x_advance as f64 * k;
        out.push(PlacedGlyph {
            gid: sg.gid,
            x: x + sg.x_offset as f64 * k,
            y: y - sg.y_offset as f64 * k,
            adv,
            sx: k,
            sy: k,
            len: 0,
            generated_text: None,
            visible: true,
            upright: upright_in_vertical(ch),
            tcy: None,
            rtl: false,
            ..base.clone()
        });
        x += adv;
    }
    (out, x)
}

/// The emphasis `character` (a sesame dot when empty) centred over `b`, in `b`'s font. A
/// character the font lacks is its missing-glyph box, unless `fallback` (fallback fonts draw
/// missing glyphs): then a fallback font draws it, or failing that a bullet.
fn kenten(db: &ScopedFonts<'_>, b: &PlacedGlyph, size: f64, character: &str, fallback: bool) -> Vec<PlacedGlyph> {
    let character = if character.is_empty() { "\u{FE45}" } else { character };
    let mut base = b.clone();
    if fallback
        && let Some(c) = character.chars().next()
        && !base.face.covers(c)
        && let Some(face) = db.fallback_for(c, base.face.id(), None)
    {
        base.face = designcraft_fonts::FaceRef::of(&face);
    }
    let (mut dot, mut w) = set(&base, character, size * SCALE, size);
    if fallback && dot.first().is_some_and(|d| d.gid == 0) {
        (dot, w) = set(b, "\u{2022}", size * SCALE, size);
    }
    // Centre the mark's ink (its advance may hold it off centre) and sit it just over the text.
    let ink = dot.first().map(|d| designcraft_fonts::FontDb::global().outline(&d.face, d.gid).bounding_box()).filter(|r| r.width() > 0.0);
    for d in &mut dot {
        match ink {
            Some(r) => {
                d.x = b.x + b.adv / 2.0 - (r.x0 + r.x1) / 2.0 * d.sx;
                d.y = -(size * 0.92) - r.y1 * d.sy;
            }
            None => d.x += b.x + (b.adv - w) / 2.0,
        }
    }
    dot
}

/// Add the ruby and kenten of a laid-out line's glyphs. `glyph_fallback`: the document draws
/// missing glyphs from fallback fonts (a missing font's substitute always does).
pub(crate) fn annotate(db: &ScopedFonts<'_>, styles: &[RunStyle], line: &mut Vec<PlacedGlyph>, glyph_fallback: bool) {
    let style = |g: &PlacedGlyph| styles.get(g.style as usize);
    if !line.iter().any(|g| style(g).is_some_and(|s| s.ruby.is_some() || s.kenten)) {
        return;
    }
    let mut extra = Vec::new();
    let mut i = 0;
    while i < line.len() {
        let g = &line[i];
        let Some(st) = style(g).filter(|_| g.len > 0 && g.visible) else {
            i += 1;
            continue;
        };
        if st.kenten && g.adv > 0.0 {
            extra.extend(kenten(db, g, st.size, &st.kenten_character, glyph_fallback || st.missing_font));
        }
        let Some(text) = &st.ruby else {
            i += 1;
            continue;
        };
        // The group: following glyphs with the same ruby.
        let mut j = i + 1;
        while j < line.len() && (line[j].len == 0 || style(&line[j]).is_some_and(|s| s.ruby.as_ref() == Some(text))) {
            j += 1;
        }
        let base: Vec<&PlacedGlyph> = line[i..j].iter().filter(|b| b.len > 0).collect();
        let x0 = base.iter().map(|b| b.x).fold(f64::MAX, f64::min);
        let x1 = base.iter().map(|b| b.x + b.adv).fold(f64::MIN, f64::max);
        let (mut glyphs, w) = set(g, text, st.size * SCALE, st.size);
        let n = glyphs.len().max(1) as f64;
        if w < x1 - x0 {
            // Shorter than the base: spread out, half a gap at each end (the 1-2-1 rule).
            let gap = (x1 - x0 - w) / n;
            for (k, r) in glyphs.iter_mut().enumerate() {
                r.x += x0 + gap * (k as f64 + 0.5);
            }
        } else {
            // Longer: centred over the base.
            let start = (x0 + x1 - w) / 2.0;
            for r in &mut glyphs {
                r.x += start;
            }
        }
        extra.extend(glyphs);
        // Kenten on the rest of the group.
        for b in line[i + 1..j].iter().filter(|b| st.kenten && b.len > 0 && b.adv > 0.0) {
            extra.extend(kenten(db, b, st.size, &st.kenten_character, glyph_fallback || st.missing_font));
        }
        i = j;
    }
    line.extend(extra);
}
