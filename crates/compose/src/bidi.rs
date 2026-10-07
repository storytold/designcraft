//! Paragraph-wide Unicode bidi input, retaining cluster offsets through line breaking.
use super::shape::Glyph;
use designcraft_doc::arabic::CharacterDirection as D;

pub fn paragraph_text(glyphs: &mut [Glyph], source: &str) -> String {
    let mut text = String::new();
    let mut direction = D::Default;
    let mut previous: Option<(usize, usize, usize)> = None;
    for g in glyphs {
        if g.len == 0
            && let Some((byte, start, end)) = previous
            && byte == g.byte
        {
            g.bidi_offset = start;
            g.bidi_end = end;
            continue;
        }
        if g.character_direction != direction {
            if direction != D::Default {
                text.push('\u{202C}');
            }
            direction = g.character_direction;
            match direction {
                D::Default => {}
                D::LeftToRight => text.push('\u{202D}'),
                D::RightToLeft => text.push('\u{202E}'),
            }
        }
        g.bidi_offset = text.len();
        if g.len > 0 && g.display_char == g.ch {
            match source.get(g.byte..g.byte.saturating_add(g.len)) {
                Some(s) => text.push_str(s),
                None => text.push(g.display_char),
            }
        } else {
            text.push(g.display_char);
        }
        g.bidi_end = text.len();
        previous = (g.len > 0).then_some((g.byte, g.bidi_offset, g.bidi_end));
    }
    if direction != D::Default {
        text.push('\u{202C}');
    }
    text
}

/// Correct single-character mirrored clusters when their paragraph direction differs
/// from the independently shaped run. Keep kerning/tracking in the advance delta.
pub fn resolve_mirroring(glyphs: &mut [Glyph], info: &unicode_bidi::BidiInfo<'_>) {
    for i in 0..glyphs.len() {
        let g = &glyphs[i];
        if g.len != g.display_char.len_utf8() || glyphs.get(i + 1).is_some_and(|next| next.byte == g.byte) {
            continue;
        }
        let rtl = info.levels.get(g.bidi_offset).is_some_and(|l| l.is_rtl());
        if rtl == g.shaping_rtl {
            continue;
        }
        let Some(mirror) = unicode_bidi_mirroring::get_mirrored(g.display_char) else { continue };
        let gid = g.face.glyph_for(if rtl { mirror } else { g.display_char });
        if gid != 0 {
            let g = &mut glyphs[i];
            g.adv += (g.face.advance(gid) - g.face.advance(g.gid)) * g.sx;
            g.gid = gid;
            g.shaping_rtl = rtl;
        }
    }
}
