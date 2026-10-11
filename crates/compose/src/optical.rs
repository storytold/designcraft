//! Optical kerning: pair spacing measured from the glyph outlines, for text whose kerning is
//! set to Optical.
//!
//! The font's own kern pairs are not used (Optical replaces Metrics, as in InDesign). Instead,
//! each glyph's ink is sampled into horizontal bands across the em, and for each pair the gap
//! between the left glyph's right edge and the right glyph's left edge is read in the bands where
//! both have ink. The pair is then moved part of the way towards the font's natural stem-to-stem
//! gap (the gap of the pair `nn`, or of `HH` then `oo` when the font lacks them), never closer
//! than a fraction of it, and never by more than a small part of the em. `nn` itself stays as the
//! font set it; diagonal pairs (`AV`, `To`) and letter-punctuation pairs (`r.`, `f,`) close up;
//! round pairs such as `oo`, and pairs that already touch, open slightly.
//!
//! Profiles are cached per (face, glyph) and the reference gap per face, so body text in Optical
//! costs one outline walk per distinct glyph.

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
/// How far a pair moves towards the reference gap (1 = all the way).
const PULL: f64 = 0.6;
/// The ink gap a pair keeps at least, as a fraction of the reference gap.
const FLOOR: f64 = 0.3;
/// Tightest and loosest adjustment, in em.
const MAX_TIGHTEN: f64 = -0.12;
const MAX_LOOSEN: f64 = 0.06;
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

    /// Mean and smallest ink gap between `self` (set first, advancing `adv`) and `next`, over the
    /// bands where both have ink; `None` when they share fewer than two bands.
    pub fn gap(&self, next: &Profile, adv: f64) -> Option<(f64, f64)> {
        let gaps: Vec<f64> = self.bands.iter().zip(next.bands.iter()).filter_map(|(a, b)| Some(adv + b.as_ref()?.0 - a.as_ref()?.1)).collect();
        if gaps.len() < 2 {
            return None;
        }
        let mean = gaps.iter().sum::<f64>() / gaps.len() as f64;
        let min = gaps.iter().copied().fold(f64::INFINITY, f64::min);
        Some((mean, min))
    }
}

static PROFILES: LazyLock<RwLock<HashMap<(u32, u32), Arc<Profile>>>> = LazyLock::new(Default::default);
static REFERENCES: LazyLock<RwLock<HashMap<u32, Option<f64>>>> = LazyLock::new(Default::default);

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

/// The font's natural gap between two straight stems, in font units: the mean ink gap of `nn`
/// (or of `HH`, then `oo`, when the font lacks them). `None` when the font has none of them.
fn reference_gap(db: &ScopedFonts<'_>, face: &FontFace) -> Option<f64> {
    let id = face.id();
    if let Some(r) = REFERENCES.read().unwrap_or_else(PoisonError::into_inner).get(&id) {
        return *r;
    }
    let found = ['n', 'H', 'o'].into_iter().find_map(|c| {
        let gid = face.glyph_for(c);
        if gid == 0 {
            return None;
        }
        let p = profile_of(db, face, gid);
        let (mean, _) = p.gap(&p, face.advance(gid))?;
        (mean > 0.0 && mean.is_finite()).then_some(mean)
    });
    REFERENCES.write().unwrap_or_else(PoisonError::into_inner).insert(id, found);
    found
}

/// Whether `c` takes part in optical kerning: letters of the Latin, Greek and Cyrillic scripts,
/// digits and Western punctuation. Spaces, marks, joined and ideographic scripts keep their
/// font spacing.
pub fn kernable(c: char) -> bool {
    use unicode_script::{Script, UnicodeScript};
    if c.is_ascii_digit() || c.is_ascii_punctuation() || ('\u{2010}'..='\u{2027}').contains(&c) {
        return true;
    }
    c.is_alphabetic() && matches!(c.script(), Script::Latin | Script::Greek | Script::Cyrillic)
}

/// The adjustment for a pair, in font units: how far the first glyph's advance moves.
fn adjustment(reference: f64, mean: f64, min: f64, upem: f64) -> f64 {
    let mut adj = (reference - mean) * PULL;
    let floor = FLOOR * reference;
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
        let Some(reference) = reference_gap(db, face) else { continue };
        let (pa, pb) = (profile_of(db, face, a.gid), profile_of(db, face, b.gid));
        // The pair's distance as shaped (features such as `palt` change it), without tracking,
        // plus the glyphs' own x offsets; in font units.
        let adv = (a.adv - tracking + b.dx - a.dx) / a.sx;
        let Some((mean, min)) = pa.gap(&pb, adv) else { continue };
        let adj = adjustment(reference, mean, min, face.upem);
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
        // Two such stems 300 apart: the gap is 200 in every band.
        let (mean, min) = p.gap(&p, 300.0).unwrap();
        assert!((mean - 200.0).abs() < 1e-9 && (min - 200.0).abs() < 1e-9);
    }

    #[test]
    fn no_shared_band_no_gap() {
        let high = Profile::of(&rect(0.0, -900.0, 100.0, -700.0), 1000.0);
        let low = Profile::of(&rect(0.0, -100.0, 100.0, 0.0), 1000.0);
        assert!(high.gap(&low, 200.0).is_none());
    }

    #[test]
    fn adjustment_is_bounded_and_keeps_a_floor() {
        // Reference 200: a pair already at 200 doesn't move.
        assert!(adjustment(200.0, 200.0, 200.0, 1000.0).abs() < 1e-9);
        // A wide pair tightens by PULL of the excess, up to the cap.
        assert!((adjustment(200.0, 300.0, 300.0, 1000.0) + 60.0).abs() < 1e-9);
        assert!((adjustment(200.0, 1000.0, 1000.0, 1000.0) + 120.0).abs() < 1e-9);
        // Tightening never takes the smallest gap under the floor.
        assert!((adjustment(200.0, 300.0, 70.0, 1000.0) + 10.0).abs() < 1e-9);
        // A touching pair opens, up to the cap.
        assert!((adjustment(200.0, 100.0, -50.0, 1000.0) - 60.0).abs() < 1e-9);
    }

    #[test]
    fn kernable_characters() {
        assert!(kernable('A') && kernable('é') && kernable('Ж') && kernable('Ω') && kernable('7') && kernable('.') && kernable('“'));
        assert!(!kernable(' ') && !kernable('\t') && !kernable('漢') && !kernable('ب') && !kernable('\u{301}'));
    }
}
