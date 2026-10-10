//! Shatai (斜体): the oblique of Japanese phototypesetting. The character's em box is compressed
//! by the magnification across the direction at the shatai angle and kept at full size along it,
//! about the em box centre, which slants the character. Adjust Rotation turns the result back so
//! the em box edge along the line stays along the line; Adjust Tsume fits the advance to the
//! compressed em box.
//!
//! The matrix matches what InDesign prints (measured from a public sample and its print PDF, as
//! recorded by the clean-room `indd-to-idml` format notes): 20 % at 60° gives the text matrix
//! (0.850, 0.0866, 0.0866, 0.950); 10 % at 60° with Adjust Rotation in vertical text gives
//! (0.9222, 0.0843, 0, 0.9759). The pivot (em box centre) and the horizontal-text rotation (the
//! glyph's horizontal axis stays horizontal) are not measured.

use designcraft_doc::CharProps;
use designcraft_geom::{Affine, Point};

use crate::{PlacedGlyph, RunStyle};

/// A run's shatai, resolved for layout ([`RunStyle::shatai`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shatai {
    /// Compression across the angle, as a fraction of the em (0 < m ≤ 0.9).
    pub magnification: f64,
    /// Degrees counter-clockwise from the line direction, in [0, 180).
    pub angle: f64,
    pub adjust_rotation: bool,
    pub adjust_tsume: bool,
}

/// Largest compression kept: the em box never collapses to a line.
const MAX_MAGNIFICATION: f64 = 0.9;

impl Shatai {
    /// The shatai of text with `p` (`None` when off).
    pub fn of(p: &CharProps) -> Option<Shatai> {
        let m = p.shatai_magnification;
        if !m.is_finite() || m <= 0.0 {
            return None;
        }
        let angle = if p.shatai_angle.is_finite() { p.shatai_angle.rem_euclid(180.0) } else { 45.0 };
        Some(Shatai {
            magnification: (m / 100.0).min(MAX_MAGNIFICATION),
            angle,
            adjust_rotation: p.shatai_adjust_rotation,
            adjust_tsume: p.shatai_adjust_tsume,
        })
    }

    /// The linear part in glyph space, y up, as PDF matrix values `[a, b, c, d]` (x' = a·x + c·y,
    /// y' = b·x + d·y). `along_y`: the line runs along the glyph's y axis (an upright glyph in
    /// vertical text), which Adjust Rotation keeps vertical; otherwise it keeps the x axis
    /// horizontal.
    pub fn linear(&self, along_y: bool) -> [f64; 4] {
        let (s, c) = self.angle.to_radians().sin_cos();
        let m = self.magnification;
        // Identity minus m·v·vᵀ, v the unit normal of the angle's direction.
        let (a, b, cc, d) = (1.0 - m * s * s, m * s * c, m * s * c, 1.0 - m * c * c);
        if !self.adjust_rotation {
            return [a, b, cc, d];
        }
        // Turn by φ so the chosen axis' image lies on that axis again.
        let phi = if along_y { cc.atan2(d) } else { -b.atan2(a) };
        let (sp, cp) = phi.sin_cos();
        [cp * a - sp * b, sp * a + cp * b, cp * cc - sp * d, sp * cc + cp * d]
    }

    /// The em box's extent along the line after the transform, over its extent before: the
    /// advance factor of Adjust Tsume. `w`, `h`: the em box's width and height.
    pub fn extent(&self, along_y: bool, w: f64, h: f64) -> f64 {
        let [a, b, c, d] = self.linear(along_y);
        if along_y {
            if h <= 0.0 { 1.0 } else { (b.abs() * w + d.abs() * h) / h }
        } else if w <= 0.0 {
            1.0
        } else {
            (a.abs() * w + c.abs() * h) / w
        }
    }
}

impl RunStyle {
    /// Shatai of glyph `g` on a line at `baseline`, in the line's space (y down): applied after
    /// the glyph is drawn at its place and before a vertical frame's upright turn. `None` without
    /// shatai.
    pub fn shatai_xf(&self, g: &PlacedGlyph, baseline: f64) -> Option<Affine> {
        let sh = self.shatai?;
        let [a, b, c, d] = sh.linear(g.upright);
        // y up → y down: negate the off-diagonal terms.
        let lin = Affine::new([a, -b, -c, d, 0.0, 0.0]);
        let (top, bottom) = g.face.em_box();
        let centre = Point::new(g.x + g.face.advance(g.gid) * g.sx / 2.0, baseline + g.y - (top + bottom) / 2.0 * g.sy);
        Some(Affine::translate(centre.to_vec2()) * lin * Affine::translate(-centre.to_vec2()))
    }
}

/// Adjust Tsume: each shaped glyph of a shatai run takes the compressed em box's extent along the
/// line, centred in it.
pub(crate) fn fit_advances(run: &mut [crate::shape::Glyph], p: &CharProps) {
    let Some(sh) = Shatai::of(p).filter(|s| s.adjust_tsume) else { return };
    for g in run.iter_mut().filter(|g| g.adv > 0.0 && !g.ch.is_control() && !g.ch.is_whitespace()) {
        let (top, bottom) = g.face.em_box();
        let w = g.face.advance(g.gid) * g.sx;
        let h = (top - bottom) * g.sy;
        let along = if g.upright { h } else { w };
        let delta = along * (sh.extent(g.upright, w, h) - 1.0);
        if delta.is_finite() {
            g.adv = (g.adv + delta).max(0.0);
            g.dx += delta / 2.0;
        }
    }
}
