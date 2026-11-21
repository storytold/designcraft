//! Grid Settings (グリッド設定) against the baseline grid: which point of a line sits on a grid
//! line (grid alignment), how many grid lines a line takes (gyoudori, 行取り), and paragraph
//! gyoudori, where the paragraph as a whole takes them.
//!
//! The document has a baseline grid (document or frame), not a frame grid of character cells, so
//! each grid line stands for one line slot. A line that takes `n` grid lines holds its reference
//! point on the first of them (em box or ICF top), midway (em box centre) or on the last (roman
//! baseline, em box or ICF bottom); a paragraph-gyoudori block is placed the same way, measured
//! from its first line's reference to its last's. The next line starts on the grid line after the
//! band.

use designcraft_doc::cjk::CharacterAlignment as Reference;

use crate::shape::Glyph;

/// Most grid lines one line or paragraph takes (input-derived).
pub(crate) const MAX_GYOUDORI: u32 = 100;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Anchor {
    First,
    Middle,
    Last,
}

fn anchor(r: Reference) -> Anchor {
    match r {
        Reference::EmTop | Reference::IcfTop => Anchor::First,
        Reference::EmCenter => Anchor::Middle,
        Reference::Baseline | Reference::EmBottom | Reference::IcfBottom => Anchor::Last,
    }
}

/// The line's grid reference point below its baseline (negative = above), from its largest
/// character's em box ([`crate::cjk_em_box`], as the leading model measures it) or ICF box
/// ([`crate::cjk_icf_box`]).
pub(crate) fn reference_offset(line: &[Glyph], r: Reference) -> f64 {
    if r == Reference::Baseline {
        return 0.0;
    }
    let Some(g) = line.iter().filter(|g| g.adv > 0.0).max_by(|a, b| a.size.total_cmp(&b.size)) else { return 0.0 };
    let (top, bottom) = crate::cjk_em_box(g);
    let (icf_top, icf_bottom) = crate::cjk_icf_box(g);
    match r {
        Reference::Baseline => 0.0,
        Reference::EmTop => top,
        Reference::EmCenter => (top + bottom) / 2.0,
        Reference::EmBottom => bottom,
        Reference::IcfTop => icf_top,
        Reference::IcfBottom => icf_bottom,
    }
}

/// The point of a line that goes on a frame grid's reference `r`: ICF Top and ICF Bottom put the
/// line's em box edge on the cell's ICF edge (InDesign does so whatever the text size), the other
/// references are the line's own.
pub(crate) fn frame_line_reference(r: Reference) -> Reference {
    match r {
        Reference::IcfTop => Reference::EmTop,
        Reference::IcfBottom => Reference::EmBottom,
        r => r,
    }
}

impl crate::FrameRows {
    /// The grid [`place`] takes for reference `r`: the first row's reference point and the row
    /// pitch. The cell is an em box of the line's largest face (the grid's font once Apply Grid
    /// Format has set it): its roman baseline and ICF sit where that face puts them.
    pub(crate) fn grid_for(&self, line: &[Glyph], r: Reference) -> (f64, f64) {
        let face = line.iter().filter(|g| g.adv > 0.0).max_by(|a, b| a.size.total_cmp(&b.size)).map(|g| {
            let (top, bottom) = crate::cjk_em_box(g);
            let (icf_top, icf_bottom) = crate::cjk_icf_box(g);
            let em = (bottom - top).max(1e-9);
            ((0.0 - top) / em, (icf_top - top) / em, (icf_bottom - top) / em)
        });
        // Without a glyph: the em box 0.88 em above the baseline and an ICF inset by 5 %.
        let (roman, icf_top, icf_bottom) = face.filter(|f| f.0.is_finite() && f.1.is_finite() && f.2.is_finite()).unwrap_or((0.88, 0.05, 0.95));
        let at = match r {
            Reference::EmTop => 0.0,
            Reference::EmCenter => 0.5,
            Reference::EmBottom => 1.0,
            Reference::Baseline => roman,
            Reference::IcfTop => icf_top,
            Reference::IcfBottom => icf_bottom,
        };
        (self.top + at.clamp(0.0, 1.0) * self.cell, self.pitch)
    }

    /// Rows a line takes when its paragraph leaves gyoudori to the grid (auto): as many as its
    /// em box of height `em` needs.
    pub(crate) fn rows_for(&self, em: f64) -> u32 {
        let extra = if self.pitch > 0.0 && em.is_finite() { ((em - self.cell) / self.pitch - 1e-6).ceil().max(0.0) } else { 0.0 };
        (extra.min(f64::from(MAX_GYOUDORI)) as u32).saturating_add(1).min(MAX_GYOUDORI)
    }
}

/// Index of the first grid line at or below `y`.
fn index_at(y: f64, start: f64, inc: f64) -> f64 {
    ((y - start) / inc - 1e-6).ceil()
}

/// Grid lines a block of reference points `span` apart needs, at least `n`.
pub(crate) fn lines_for(span: f64, inc: f64, n: u32) -> u32 {
    let need = if inc > 0.0 && span.is_finite() && span > 0.0 { (span / inc + 1e-6).floor() + 1.0 } else { 1.0 };
    let need = need.min(f64::from(MAX_GYOUDORI)) as u32;
    need.max(n).clamp(1, MAX_GYOUDORI)
}

/// What a band of grid lines asks of [`place`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct Band {
    /// The block's first reference point without the grid.
    pub natural: f64,
    /// First grid line not taken by earlier lines of the column (`None` at its top).
    pub free: Option<f64>,
    /// Space before still to add (paragraph space before, rounded up to whole grid lines).
    pub pending: f64,
    /// Gyoudori is set: the band starts right after the previous one, whatever the leading.
    pub fixed: bool,
    /// Grid lines the band takes (1 for an ordinary line).
    pub lines: u32,
    /// From the block's first reference point to its last (0 for one line).
    pub span: f64,
    /// Frame grids: the band is the first whose reference point (first row, middle or last row)
    /// is at or below the natural one, so a two-row heading takes the first two rows. On the
    /// baseline grid the band's first grid line is.
    pub at_reference: bool,
}

/// Place a band on the grid `(start, increment)`: the block's first reference point, and the grid
/// line after the band (the next line's first free grid line).
pub(crate) fn place(grid: (f64, f64), r: Reference, band: Band) -> (f64, f64) {
    let (start, inc) = grid;
    let lines = f64::from(band.lines.clamp(1, MAX_GYOUDORI));
    let room = ((lines - 1.0) * inc - band.span).max(0.0);
    let offset = match anchor(r) {
        Anchor::First => 0.0,
        Anchor::Middle => room / 2.0,
        Anchor::Last => room,
    };
    let from_natural = index_at(band.natural - if band.at_reference { offset } else { 0.0 }, start, inc);
    let k = match band.free {
        Some(y) if band.fixed => index_at(y + band.pending.max(0.0), start, inc),
        Some(y) => from_natural.max(index_at(y, start, inc)),
        None => from_natural,
    };
    let first = start + k * inc;
    (first + offset, first + lines * inc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_hold_their_reference_first_midway_or_last() {
        let band = Band { natural: 25.0, free: None, pending: 0.0, fixed: false, lines: 1, span: 0.0, at_reference: false };
        // One line: the next grid line at or below the natural position, whatever the reference.
        assert_eq!(place((0.0, 12.0), Reference::Baseline, band), (36.0, 48.0));
        assert_eq!(place((0.0, 12.0), Reference::EmTop, band), (36.0, 48.0));
        let two = Band { lines: 2, ..band };
        assert_eq!(place((0.0, 12.0), Reference::EmTop, two), (36.0, 60.0));
        assert_eq!(place((0.0, 12.0), Reference::EmCenter, two), (42.0, 60.0));
        assert_eq!(place((0.0, 12.0), Reference::Baseline, two), (48.0, 60.0));
        // Gyoudori starts on the first free grid line, not where its leading would put it.
        let fixed = Band { natural: 70.0, free: Some(48.0), fixed: true, ..two };
        assert_eq!(place((0.0, 12.0), Reference::EmCenter, fixed).0, 54.0);
        // A block of two lines 14 pt apart in three grid lines: centred in the room left.
        let block = Band { lines: lines_for(14.0, 12.0, 3), span: 14.0, ..band };
        assert_eq!(block.lines, 3);
        assert_eq!(place((0.0, 12.0), Reference::EmCenter, block).0, 36.0 + 5.0);
        assert_eq!(lines_for(1e300, 12.0, 0), MAX_GYOUDORI);
        assert_eq!(lines_for(f64::NAN, 12.0, 0), 1);
    }

    #[test]
    fn a_frame_grid_band_is_the_first_whose_reference_is_at_or_below_the_natural_one() {
        // Rows 18 apart; a 20 pt line's em centre is 10 below the frame top, the centre of rows
        // 0–1 is 15: the line takes rows 0–1 (InDesign), not rows 1–2. The second number is the
        // next band's first reference point (row 2's centre).
        let band = Band { natural: 10.0, free: None, pending: 0.0, fixed: false, lines: 2, span: 0.0, at_reference: true };
        assert_eq!(place((6.0, 18.0), Reference::EmCenter, band), (15.0, 42.0));
        assert_eq!(place((6.0, 18.0), Reference::EmCenter, Band { at_reference: false, ..band }), (33.0, 60.0));
        // The em bottom of a 30 pt line at 30 is the bottom of rows 0–1.
        let bottom = Band { natural: 30.0, ..band };
        assert_eq!(place((12.0, 18.0), Reference::EmBottom, bottom), (30.0, 48.0));
    }
}
