//! Optical kerning: pair spacing measured from the glyph outlines, for text whose kerning is
//! set to Optical.
//!
//! The font's own kern pairs are not used (Optical replaces Metrics, as in InDesign). Instead,
//! each glyph's ink is sampled into horizontal bands across the em, and for each pair the white
//! between the two glyphs is read band by band from about the cap height to the baseline: the gap
//! between the inks where both glyphs have some, a glyph's own side bearing plus some depth where
//! only one has (the open side of `T`, `r`, `.`). Each band counts up to a limited depth, so a
//! deep hollow counts little more than a moderately open one. The pair then moves towards one
//! target, the same fraction of the em in every font, a little wider for wide fonts (measured by
//! the advance of `n`). Open pairs close up harder than tight ones open; a pair never comes closer
//! than a small gap between the inks, and never moves by more than part of the em.
//!
//! The constants are fitted to how InDesign sets ten text faces (sans and serif, regular and
//! bold) under Optical, read from its kerning values: most text pairs, `nn` included, close up a
//! little, `AV`, `To` and `P.` close up a lot, and pairs whose inks nearly touch (`VW`) open.
//!
//! Profiles are cached per (face, glyph) and the target per face, so body text in Optical costs
//! one outline walk per distinct glyph.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, PoisonError, RwLock};

use designcraft_fonts::{FontFace, ScopedFonts};
use designcraft_geom::{BezPath, PathEl, Point};

use crate::shape::Glyph;

/// Horizontal bands across the em, from `TOP` to `BOTTOM` em (outlines are y-down: negative is
/// above the baseline).
const BANDS: usize = 28;
const TOP: f64 = -1.0;
const BOTTOM: f64 = 0.3;
/// The bands whose centre lies in this zone are measured (em, y-down): cap height to baseline.
const ZONE: (f64, f64) = (-0.868, 0.027);
/// The white a band counts at most, in em, and the deeper limit of the open-pair term.
const DEPTH: f64 = 0.218;
const DEEP: f64 = 0.247;
/// A band where only one glyph has ink counts that glyph's side bearing plus this many `DEEP`.
const OPEN_SIDE: f64 = 1.26;
/// The target white in em for a font whose `n` advances `TARGET_N` em; it grows by
/// `TARGET_WIDTH` per em of a wider `n`.
const TARGET: f64 = 0.1045;
const TARGET_N: f64 = 0.55;
const TARGET_WIDTH: f64 = 0.251;
/// How far a pair moves towards the target: closing an open pair, opening a tight one.
const CLOSE: f64 = 0.141;
const OPEN: f64 = 0.289;
/// The extra close-up per em of white the bands hold between `DEPTH` and `DEEP`.
const DEEP_CLOSE: f64 = 2.0;
/// The ink gap a pair keeps at least, in em.
const FLOOR: f64 = 0.05;
/// Tightest and loosest adjustment, in em.
const MAX_TIGHTEN: f64 = -0.145;
const MAX_LOOSEN: f64 = 0.024;
/// A glyph wider or taller than this many em is not measured.
const MAX_EXTENT: f64 = 8.0;
const PROFILE_CACHE_MAX: usize = 20_000;

/// A glyph's ink extent per band, in font units: `None` where the band holds no ink.
#[derive(Clone, Debug)]
pub struct Profile {
    bands: [Option<(f64, f64)>; BANDS],
}

impl Profile {
    /// Sample `path` (font units, y-down) into bands of the em.
    pub fn of(path: &BezPath, upem: f64) -> Profile {
        let mut bands = [None; BANDS];
        if upem.is_nan() || upem.is_infinite() || upem <= 0.0 {
            return Profile { bands };
        }
        // A glyph drawn far outside its em is no letter (a hostile font): no profile, no kerning.
        let bbox = designcraft_geom::Shape::bounding_box(path);
        if !(bbox.x0.is_finite() && bbox.y0.is_finite() && bbox.x1.is_finite() && bbox.y1.is_finite())
            || bbox.width() > MAX_EXTENT * upem
            || bbox.height() > MAX_EXTENT * upem
        {
            return Profile { bands };
        }
        let y0 = TOP * upem;
        let h = (BOTTOM - TOP) * upem / BANDS as f64;
        let mut add = |a: Point, b: Point| {
            let (ya, yb) = (a.y.min(b.y), a.y.max(b.y));
            let first = ((ya - y0) / h).floor().max(0.0);
            let last = ((yb - y0) / h).floor().min(BANDS as f64 - 1.0);
            if first > last {
                return;
            }
            for i in first as usize..=last as usize {
                let (lo, hi) = (y0 + i as f64 * h, y0 + (i + 1) as f64 * h);
                // The segment is straight: its x extent within the band is at the clipped ends.
                let (xa, xb) = if (b.y - a.y).abs() < 1e-9 {
                    (a.x, b.x)
                } else {
                    let x_at = |y: f64| a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x);
                    (x_at(ya.max(lo)), x_at(yb.min(hi)))
                };
                let (lo_x, hi_x) = (xa.min(xb), xa.max(xb));
                if let Some(band) = bands.get_mut(i) {
                    *band = match *band {
                        None => Some((lo_x, hi_x)),
                        Some((l, r)) => Some((l.min(lo_x), r.max(hi_x))),
                    };
                }
            }
        };
        // Curves are flattened to a fraction of a band: finer makes no difference to the bands.
        let (mut cur, mut start) = (Point::ZERO, Point::ZERO);
        designcraft_geom::kurbo::flatten(path.iter(), h / 4.0, |el| match el {
            PathEl::MoveTo(p) => {
                cur = p;
                start = p;
            }
            PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => {
                add(cur, p);
                cur = p;
            }
            PathEl::ClosePath => {
                add(cur, start);
                cur = start;
            }
        });
        Profile { bands }
    }

    /// The white between `self` (set first, advancing `adv`) and `next` over the measured zone, in
    /// font units: the mean over the bands of the white up to `DEPTH`, the same up to `DEEP`, and
    /// the smallest gap between the inks (infinite where they share no band). A band where one
    /// glyph has no ink counts the other's side bearing and some depth; `None` when neither glyph
    /// has ink in the zone.
    pub fn white(&self, next: &Profile, adv: f64, upem: f64) -> Option<(f64, f64, f64)> {
        let (depth, deep) = (DEPTH * upem, DEEP * upem);
        let (mut near, mut far, mut n, mut min) = (0.0, 0.0, 0usize, f64::INFINITY);
        for (i, (a, b)) in self.bands.iter().zip(next.bands.iter()).enumerate() {
            let centre = TOP + (i as f64 + 0.5) * (BOTTOM - TOP) / BANDS as f64;
            if !(ZONE.0..=ZONE.1).contains(&centre) {
                continue;
            }
            let white = match (a, b) {
                (Some(a), Some(b)) => {
                    let gap = adv + b.0 - a.1;
                    min = min.min(gap);
                    gap
                }
                (Some(a), None) => adv - a.1 + OPEN_SIDE * deep,
                (None, Some(b)) => b.0 + OPEN_SIDE * deep,
                (None, None) => continue,
            };
            near += white.min(depth);
            far += white.min(deep);
            n += 1;
        }
        (n > 0).then(|| (near / n as f64, far / n as f64, min))
    }
}

static PROFILES: LazyLock<RwLock<HashMap<(u32, u32), Arc<Profile>>>> = LazyLock::new(Default::default);

fn profile_of(db: &ScopedFonts<'_>, face: &FontFace, gid: u32) -> Arc<Profile> {
    let key = (face.id(), gid);
    if let Some(p) = PROFILES.read().unwrap_or_else(PoisonError::into_inner).get(&key) {
        return p.clone();
    }
    let p = Arc::new(Profile::of(&db.outline(face, gid), face.upem));
    let mut w = PROFILES.write().unwrap_or_else(PoisonError::into_inner);
    if w.len() >= PROFILE_CACHE_MAX {
        w.clear();
    }
    w.entry(key).or_insert(p).clone()
}

/// The white a pair moves towards, in font units: `TARGET` em, wider for a font whose `n` is
/// wider than `TARGET_N` em.
fn target(face: &FontFace) -> f64 {
    let n = face.glyph_for('n');
    let width = if n == 0 { TARGET_N } else { face.advance(n) / face.upem };
    (TARGET + TARGET_WIDTH * (width - TARGET_N)) * face.upem
}

/// Whether `c` takes part in optical kerning: letters of the Latin, Greek and Cyrillic scripts,
/// digits and Western punctuation. Spaces, marks, joined and ideographic scripts keep their
/// font spacing, as do dashes (two bars end to end have no ink gap to measure, and `——` must
/// stay one continuous rule) and the fullwidth forms, which sit on the CJK em grid.
pub fn kernable(c: char) -> bool {
    use unicode_script::{Script, UnicodeScript};
    if is_dash(c) || ('\u{FF00}'..='\u{FFEF}').contains(&c) {
        return false;
    }
    if c.is_ascii_digit() || c.is_ascii_punctuation() || ('\u{2010}'..='\u{2027}').contains(&c) {
        return true;
    }
    c.is_alphabetic() && matches!(c.script(), Script::Latin | Script::Greek | Script::Cyrillic)
}

/// Dash punctuation (Unicode `Pd`) in the ranges optical kerning would otherwise touch.
fn is_dash(c: char) -> bool {
    matches!(c, '-' | '\u{2010}'..='\u{2015}' | '\u{2E3A}' | '\u{2E3B}' | '\u{FE58}' | '\u{FF0D}')
}

/// The adjustment for a pair, in font units: how far the first glyph's advance moves. `near`,
/// `far` and `min` are [`Profile::white`]'s.
fn adjustment(target: f64, near: f64, far: f64, min: f64, upem: f64) -> f64 {
    let off = near - target;
    let mut adj = if off > 0.0 { -CLOSE * off } else { -OPEN * off } - DEEP_CLOSE * (far - DEPTH * upem).max(0.0);
    let floor = FLOOR * upem;
    if min + adj < floor {
        adj = floor - min;
    }
    adj.clamp(MAX_TIGHTEN * upem, MAX_LOOSEN * upem)
}

/// Kern the pairs of one shaped run optically: the first glyph of each eligible pair gets its
/// advance adjusted. `tracking` is the tracking already added to each advance, in points.
pub(crate) fn kern_run(db: &ScopedFonts<'_>, run: &mut [Glyph], tracking: f64) {
    let eligible = |g: &Glyph| {
        g.len > 0
            && g.gid != 0
            && g.adv > 0.0
            && g.sx > 0.0
            && !g.upright
            && !g.shaping_rtl
            && g.tcy.is_none()
            && kernable(g.ch)
            && !g.face.glyph_is_mark(g.gid)
    };
    for i in 0..run.len().saturating_sub(1) {
        let Some((a, b)) = run.get(i).zip(run.get(i + 1)) else { break };
        if !eligible(a) || !eligible(b) || a.face.id() != b.face.id() {
            continue;
        }
        let face: &FontFace = &a.face;
        let (pa, pb) = (profile_of(db, face, a.gid), profile_of(db, face, b.gid));
        // The pair's distance as shaped (features such as `palt` change it), without tracking,
        // plus the glyphs' own x offsets; in font units.
        let adv = (a.adv - tracking + b.dx - a.dx) / a.sx;
        let Some((near, far, min)) = pa.white(&pb, adv, face.upem) else { continue };
        let adj = adjustment(target(face), near, far, min, face.upem);
        if adj.is_finite()
            && let Some(g) = run.get_mut(i)
        {
            g.adv += adj * g.sx;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_geom::Rect;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> BezPath {
        designcraft_geom::Shape::to_path(&Rect::new(x0, y0, x1, y1), 0.1)
    }

    #[test]
    fn profile_reads_a_stem() {
        // A stem from x 100..200, baseline to x-height (y -500..0), in a 1000 upem.
        let p = Profile::of(&rect(100.0, -500.0, 200.0, 0.0), 1000.0);
        let inked: Vec<_> = p.bands.iter().flatten().collect();
        assert!(inked.len() >= 9 && inked.len() <= 12, "{} bands", inked.len());
        assert!(inked.iter().all(|(l, r)| (*l - 100.0).abs() < 1e-9 && (*r - 200.0).abs() < 1e-9));
        // Two such stems 300 apart: the white is 200 in every band that has ink; the bands above
        // the x-height, empty in both, don't count.
        let (near, far, min) = p.white(&p, 300.0, 1000.0).unwrap();
        assert!((near - 200.0).abs() < 1e-9 && (far - 200.0).abs() < 1e-9 && (min - 200.0).abs() < 1e-9);
    }

    #[test]
    fn a_band_with_ink_on_one_side_counts_its_bearing_and_depth() {
        let high = Profile::of(&rect(0.0, -800.0, 100.0, -600.0), 1000.0);
        let low = Profile::of(&rect(0.0, -100.0, 100.0, 0.0), 1000.0);
        // No band has both inks: no smallest gap, and every inked band is open to `DEPTH`.
        let (near, far, min) = high.white(&low, 200.0, 1000.0).unwrap();
        assert!(min.is_infinite());
        assert!((near - DEPTH * 1000.0).abs() < 1e-9 && (far - DEEP * 1000.0).abs() < 1e-9);
        // Ink only below the baseline is outside the zone.
        let tail = Profile::of(&rect(0.0, 100.0, 100.0, 250.0), 1000.0);
        assert!(tail.white(&tail, 200.0, 1000.0).is_none());
    }

    #[test]
    fn adjustment_is_bounded_and_keeps_a_floor() {
        let adj = |near, far, min| adjustment(100.0, near, far, min, 1000.0);
        // A pair at the target doesn't move.
        assert!(adj(100.0, 100.0, 100.0).abs() < 1e-9);
        // An open pair closes by CLOSE of the excess, a tight one opens by OPEN of the shortfall.
        assert!((adj(200.0, 200.0, 200.0) + CLOSE * 100.0).abs() < 1e-9);
        assert!((adj(50.0, 50.0, 60.0) - OPEN * 50.0).abs() < 1e-9);
        // White deeper than DEPTH closes up harder.
        assert!((adj(218.0, 247.0, 247.0) + CLOSE * 118.0 + DEEP_CLOSE * 29.0).abs() < 1e-9);
        // Closing never takes the smallest gap under the floor; touching inks open, up to the cap.
        assert!((adj(200.0, 200.0, 60.0) + 10.0).abs() < 1e-9);
        assert!((adj(100.0, 100.0, -50.0) - MAX_LOOSEN * 1000.0).abs() < 1e-9);
        assert!((adjustment(-1000.0, 247.0, 1000.0, f64::INFINITY, 1000.0) - MAX_TIGHTEN * 1000.0).abs() < 1e-9);
    }

    #[test]
    fn kernable_characters() {
        assert!(kernable('A') && kernable('é') && kernable('Ж') && kernable('Ω') && kernable('7') && kernable('.') && kernable('“'));
        assert!(!kernable(' ') && !kernable('\t') && !kernable('漢') && !kernable('ب') && !kernable('\u{301}'));
    }

    #[test]
    fn dashes_and_fullwidth_forms_keep_their_font_spacing() {
        // `--` and `——` have no ink gap to measure and would otherwise open up; `——` is one
        // continuous dash in Chinese text.
        for c in ['-', '\u{2010}', '\u{2013}', '\u{2014}', '\u{2015}', '\u{2E3A}', '\u{FF0D}'] {
            assert!(!kernable(c), "{c:?} is a dash");
        }
        // Fullwidth Latin letters, digits and punctuation sit on the CJK em grid.
        for c in ['\u{FF21}', '\u{FF41}', '\u{FF5A}', '\u{FF11}', '\u{FF0C}'] {
            assert!(!kernable(c), "{c:?} is a fullwidth form");
        }
        // Hyphenated and dashed words still kern on their letters.
        assert!(kernable('A') && kernable('.') && kernable('\u{2019}'));
    }
}
