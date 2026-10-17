//! Frame grids: Chinese, Japanese and Korean text set in a grid of character cells.

use designcraft_doc::FrameGrid;

use crate::shape::Glyph;

/// The em of `g` along the line, in points.
fn em_of(g: &Glyph) -> f64 {
    let unit = if g.upright { g.sy } else { g.sx };
    let em = (g.face.units_per_em() * unit).abs();
    if em.is_finite() && em > 0.0 { em } else { g.size.abs().max(0.01) }
}

/// Characters a frame grid puts one to a cell: ideographs, kana, hangul, CJK punctuation and the
/// full-width forms. Western letters, numerals and spaces keep their own widths; so do controls
/// and markers, tate-chu-yoko groups and fixed-width groups.
fn on_grid(g: &Glyph) -> bool {
    let c = g.ch;
    let full_width = crate::upright_in_vertical(c) || matches!(c as u32, 0x3001..=0x303F | 0xFF01..=0xFF60 | 0xFFE0..=0xFFE6);
    let western = c.is_ascii() || c.is_whitespace();
    full_width && !western && g.adv > 0.0 && g.len > 0 && !c.is_control() && g.tcy.is_none() && !g.locked_advance && !crate::breaker::is_forced(c)
}

/// Fit a paragraph's glyphs to frame grid `grid`: full-width characters of the grid's size take
/// exactly one cell (the ink centred in it), and every grid character is followed by the grid's
/// character aki, so characters are a character pitch apart.
pub(crate) fn snap_to_grid(glyphs: &mut [Glyph], grid: &FrameGrid, vertical: bool) {
    let (along, _) = grid.cell(vertical);
    for g in glyphs.iter_mut().filter(|g| on_grid(g)) {
        let em = em_of(g);
        let same_size = (g.size - grid.size).abs() < 0.01;
        if same_size && g.adv >= 0.4 * em && g.adv <= 1.25 * em {
            g.dx += (along - g.adv) / 2.0;
            g.adv = along;
        }
        g.adv += grid.char_aki.max(-g.adv);
    }
}
