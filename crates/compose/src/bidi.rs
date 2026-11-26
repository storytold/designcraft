//! Paragraph-wide Unicode bidi input, retaining cluster offsets through line breaking.
use super::shape::Glyph;
use designcraft_doc::arabic::CharacterDirection as D;
use unicode_bidi::{BidiClass, bidi_class};

fn variable_cluster(g: &Glyph) -> Option<&str> {
    if g.list_label {
        return None;
    }
    designcraft_doc::vars::var_index(g.ch).map(|_| g.generated_text.as_deref().unwrap_or(""))
}

pub fn paragraph_text(glyphs: &mut [Glyph], source: &str) -> String {
    let mut text = String::new();
    let mut direction = D::Default;
    let mut previous: Option<(usize, usize, usize)> = None;
    for g in glyphs {
        // A variable has one source marker but may draw many clusters, each with its own
        // direction. A mark within one of those clusters shares the preceding glyph's offset.
        let variable_cluster = variable_cluster(g);
        let starts_cluster = g.len > 0 || variable_cluster.is_some_and(|cluster| !cluster.is_empty());
        if !starts_cluster
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
        let start = text.len();
        match variable_cluster {
            // Keep an offset for the empty variable's source range without creating a run.
            Some("") => text.push('\u{2060}'),
            // The variable stays inline even if its value contains a paragraph separator.
            Some(cluster) => text.extend(cluster.chars().map(|ch| if bidi_class(ch) == BidiClass::B { ' ' } else { ch })),
            None if g.len > 0 && g.display_char == g.ch => match source.get(g.byte..g.byte.saturating_add(g.len)) {
                Some(s) => text.push_str(s),
                None => text.push(g.display_char),
            },
            None => text.push(g.display_char),
        }
        g.bidi_offset = start;
        g.bidi_end = text.len();
        previous = starts_cluster.then_some((g.byte, g.bidi_offset, g.bidi_end));
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
        let next = glyphs.get(i + 1).filter(|next| next.byte == g.byte);
        let original = if let Some(cluster) = variable_cluster(g) {
            // Each variable cluster shares the source byte but has its own bidi offset.
            let mut chars = cluster.chars();
            let (Some(ch), None) = (chars.next(), chars.next()) else { continue };
            if next.is_some_and(|next| next.bidi_offset == g.bidi_offset) {
                continue;
            }
            ch
        } else {
            if g.len != g.display_char.len_utf8() || next.is_some() {
                continue;
            }
            g.display_char
        };
        let rtl = info.levels.get(g.bidi_offset).is_some_and(|l| l.is_rtl());
        if rtl == g.shaping_rtl {
            continue;
        }
        let Some(mirror) = unicode_bidi_mirroring::get_mirrored(original) else { continue };
        let gid = g.face.glyph_for(if rtl { mirror } else { original });
        if gid != 0 {
            let g = &mut glyphs[i];
            g.adv += (g.face.advance(gid) - g.face.advance(g.gid)) * g.sx;
            g.gid = gid;
            g.shaping_rtl = rtl;
        }
    }
}
