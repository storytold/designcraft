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
}

/// Place a band on the grid `(start, increment)`: the block's first reference point, and the grid
/// line after the band (the next line's first free grid line).
pub(crate) fn place(grid: (f64, f64), r: Reference, band: Band) -> (f64, f64) {
    let (start, inc) = grid;
    let from_natural = index_at(band.natural, start, inc);
    let k = match band.free {
        Some(y) if band.fixed => index_at(y + band.pending.max(0.0), start, inc),
        Some(y) => from_natural.max(index_at(y, start, inc)),
        None => from_natural,
    };
    let first = start + k * inc;
    let lines = f64::from(band.lines.clamp(1, MAX_GYOUDORI));
    let room = ((lines - 1.0) * inc - band.span).max(0.0);
    let y = match anchor(r) {
        Anchor::First => first,
        Anchor::Middle => first + room / 2.0,
        Anchor::Last => first + room,
    };
    (y, first + lines * inc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_hold_their_reference_first_midway_or_last() {
        let band = Band { natural: 25.0, free: None, pending: 0.0, fixed: false, lines: 1, span: 0.0 };
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
}
