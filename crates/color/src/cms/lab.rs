//! CIE XYZ / L*a*b* maths (D50, the ICC profile connection space) and colour differences.

use serde::{Deserialize, Serialize};

/// D50 reference white (ICC PCS), Y = 1.
pub const D50: [f32; 3] = [0.964_22, 1.0, 0.825_21];

/// A CIE L*a*b* colour (D50). L is 0..100, a/b roughly -128..127.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Lab {
    pub l: f32,
    pub a: f32,
    pub b: f32,
}

impl Lab {
    pub const fn new(l: f32, a: f32, b: f32) -> Self {
        Self { l, a, b }
    }
    /// Chroma (C*ab).
    pub fn chroma(&self) -> f32 {
        self.a.hypot(self.b)
    }
    /// Hue angle in degrees 0..360.
    pub fn hue(&self) -> f32 {
        let h = self.b.atan2(self.a).to_degrees().rem_euclid(360.0);
        // f32 rounding can turn a tiny negative angle into exactly 360.0.
        if h >= 360.0 { 0.0 } else { h }
    }
    pub fn from_lch(l: f32, c: f32, h_deg: f32) -> Self {
        let h = h_deg.to_radians();
        Self { l, a: c * h.cos(), b: c * h.sin() }
    }
}

const EPS: f32 = 216.0 / 24389.0;
const KAPPA: f32 = 24389.0 / 27.0;

pub fn xyz_to_lab(xyz: [f32; 3]) -> Lab {
    let f = |t: f32| if t > EPS { t.cbrt() } else { (KAPPA * t + 16.0) / 116.0 };
    let fx = f(xyz[0] / D50[0]);
    let fy = f(xyz[1] / D50[1]);
    let fz = f(xyz[2] / D50[2]);
    Lab { l: 116.0 * fy - 16.0, a: 500.0 * (fx - fy), b: 200.0 * (fy - fz) }
}

pub fn lab_to_xyz(lab: Lab) -> [f32; 3] {
    let fy = (lab.l + 16.0) / 116.0;
    let fx = fy + lab.a / 500.0;
    let fz = fy - lab.b / 200.0;
    let inv = |f: f32| {
        let f3 = f * f * f;
        if f3 > EPS { f3 } else { (116.0 * f - 16.0) / KAPPA }
    };
    let y = if lab.l > KAPPA * EPS { fy * fy * fy } else { lab.l / KAPPA };
    [inv(fx) * D50[0], y * D50[1], inv(fz) * D50[2]]
}

/// sRGB transfer: encoded → linear.
pub fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}
/// sRGB transfer: linear → encoded.
pub fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.max(0.0).powf(1.0 / 2.4) - 0.055 }
}

#[allow(clippy::excessive_precision)]
// ICC sRGB colorants, Bradford-adapted to D50.
const SRGB_TO_XYZ: [[f32; 3]; 3] =
    [[0.436_074_7, 0.385_064_9, 0.143_080_4], [0.222_504_5, 0.716_878_6, 0.060_616_9], [0.013_932_2, 0.097_104_5, 0.714_173_3]];
#[allow(clippy::excessive_precision)]
const XYZ_TO_SRGB: [[f32; 3]; 3] =
    [[3.133_856_1, -1.616_866_7, -0.490_614_6], [-0.978_768_4, 1.916_141_5, 0.033_454], [0.071_945_3, -0.228_991_4, 1.405_242_7]];

fn mul(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

/// Encoded sRGB (0..1) → XYZ D50.
pub fn srgb_to_xyz(rgb: [f32; 3]) -> [f32; 3] {
    mul(&SRGB_TO_XYZ, rgb.map(srgb_to_linear))
}
/// XYZ D50 → encoded sRGB, unclipped.
pub fn xyz_to_srgb_unclipped(xyz: [f32; 3]) -> [f32; 3] {
    mul(&XYZ_TO_SRGB, xyz).map(|v| v.signum() * linear_to_srgb(v.abs()))
}
/// XYZ D50 → encoded sRGB clipped to 0..1.
pub fn xyz_to_srgb(xyz: [f32; 3]) -> [f32; 3] {
    // Snap float noise so paper white is exactly white.
    mul(&XYZ_TO_SRGB, xyz).map(|v| {
        let e = linear_to_srgb(v.clamp(0.0, 1.0));
        if e > 0.99999 {
            1.0
        } else if e < 1e-6 {
            0.0
        } else {
            e
        }
    })
}
pub fn srgb_to_lab(rgb: [f32; 3]) -> Lab {
    xyz_to_lab(srgb_to_xyz(rgb))
}
pub fn lab_to_srgb(lab: Lab) -> [f32; 3] {
    xyz_to_srgb(lab_to_xyz(lab))
}
/// Linear-light sRGB (for simulations that work on linear RGB).
pub fn srgb_linear(rgb: [f32; 3]) -> [f32; 3] {
    rgb.map(srgb_to_linear)
}
pub(crate) fn mat_mul(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    mul(m, v)
}

/// CIE76 colour difference.
pub fn delta_e76(x: Lab, y: Lab) -> f32 {
    ((x.l - y.l).powi(2) + (x.a - y.a).powi(2) + (x.b - y.b).powi(2)).sqrt()
}

/// CIEDE2000 colour difference (kL = kC = kH = 1).
pub fn delta_e2000(x: Lab, y: Lab) -> f32 {
    let (l1, a1, b1) = (x.l as f64, x.a as f64, x.b as f64);
    let (l2, a2, b2) = (y.l as f64, y.a as f64, y.b as f64);
    let c1 = a1.hypot(b1);
    let c2 = a2.hypot(b2);
    let cm = (c1 + c2) / 2.0;
    let g = 0.5 * (1.0 - (cm.powi(7) / (cm.powi(7) + 25f64.powi(7))).sqrt());
    let a1p = a1 * (1.0 + g);
    let a2p = a2 * (1.0 + g);
    let c1p = a1p.hypot(b1);
    let c2p = a2p.hypot(b2);
    let hp = |b: f64, a: f64| if b == 0.0 && a == 0.0 { 0.0 } else { b.atan2(a).to_degrees().rem_euclid(360.0) };
    let h1p = hp(b1, a1p);
    let h2p = hp(b2, a2p);
    let dlp = l2 - l1;
    let dcp = c2p - c1p;
    let dhp = if c1p * c2p == 0.0 {
        0.0
    } else if (h2p - h1p).abs() <= 180.0 {
        h2p - h1p
    } else if h2p - h1p > 180.0 {
        h2p - h1p - 360.0
    } else {
        h2p - h1p + 360.0
    };
    let dhp_big = 2.0 * (c1p * c2p).sqrt() * (dhp.to_radians() / 2.0).sin();
    let lpm = (l1 + l2) / 2.0;
    let cpm = (c1p + c2p) / 2.0;
    let hpm = if c1p * c2p == 0.0 {
        h1p + h2p
    } else if (h1p - h2p).abs() <= 180.0 {
        (h1p + h2p) / 2.0
    } else if h1p + h2p < 360.0 {
        (h1p + h2p + 360.0) / 2.0
    } else {
        (h1p + h2p - 360.0) / 2.0
    };
    let t = 1.0 - 0.17 * (hpm - 30.0).to_radians().cos() + 0.24 * (2.0 * hpm).to_radians().cos() + 0.32 * (3.0 * hpm + 6.0).to_radians().cos()
        - 0.20 * (4.0 * hpm - 63.0).to_radians().cos();
    let dtheta = 30.0 * (-((hpm - 275.0) / 25.0).powi(2)).exp();
    let rc = 2.0 * (cpm.powi(7) / (cpm.powi(7) + 25f64.powi(7))).sqrt();
    let sl = 1.0 + 0.015 * (lpm - 50.0).powi(2) / (20.0 + (lpm - 50.0).powi(2)).sqrt();
    let sc = 1.0 + 0.045 * cpm;
    let sh = 1.0 + 0.015 * cpm * t;
    let rt = -(2.0 * dtheta).to_radians().sin() * rc;
    let v = (dlp / sl).powi(2) + (dcp / sc).powi(2) + (dhp_big / sh).powi(2) + rt * (dcp / sc) * (dhp_big / sh);
    v.max(0.0).sqrt() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hue_tiny_negative_b_stays_below_360() {
        let h = Lab::new(50.0, 10.0, -1e-6).hue();
        assert!((0.0..360.0).contains(&h), "hue {h}");
        assert_eq!(h, 0.0);
    }

    #[test]
    fn white_and_black() {
        let w = srgb_to_lab([1.0, 1.0, 1.0]);
        assert!((w.l - 100.0).abs() < 0.05 && w.a.abs() < 0.05 && w.b.abs() < 0.05, "{w:?}");
        let k = srgb_to_lab([0.0, 0.0, 0.0]);
        assert!(k.l.abs() < 0.01);
    }

    #[test]
    fn lab_roundtrip() {
        for rgb in [[0.2, 0.4, 0.6], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.9, 0.9, 0.1]] {
            let back = lab_to_srgb(srgb_to_lab(rgb));
            for i in 0..3 {
                assert!((back[i] - rgb[i]).abs() < 1e-3, "{rgb:?} → {back:?}");
            }
        }
    }

    #[test]
    fn ciede2000_reference_pairs() {
        // Sharma, Wu & Dalal (2005) test data, pairs 1 and 7.
        let d = delta_e2000(Lab::new(50.0, 2.6772, -79.7751), Lab::new(50.0, 0.0, -82.7485));
        assert!((d - 2.0425).abs() < 1e-3, "{d}");
        let d = delta_e2000(Lab::new(50.0, 0.0, 0.0), Lab::new(50.0, -1.0, 2.0));
        assert!((d - 2.3669).abs() < 1e-3, "{d}");
    }
}
