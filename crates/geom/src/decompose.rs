//! Transform panel values: an affine split into scale, rotation and shear the way InDesign shows
//! them (rotation counter-clockwise on screen, shear as an angle along x).

use kurbo::Affine;

/// `m`'s linear part as rotation · shear · scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decomposed {
    /// Scale factors (1 = 100%); `scale_y` is negative for a flipped transform.
    pub scale_x: f64,
    pub scale_y: f64,
    /// Degrees, counter-clockwise on screen, in (-180, 180].
    pub rotation: f64,
    /// Degrees (as `transform.shear` takes them).
    pub shear: f64,
}

pub fn decompose(m: Affine) -> Decomposed {
    let [a, b, c, d, _, _] = m.as_coeffs();
    let sx = a.hypot(b);
    if sx < 1e-12 {
        return Decomposed { scale_x: 0.0, scale_y: 0.0, rotation: 0.0, shear: 0.0 };
    }
    let det = a * d - b * c;
    let rotation = (-b).atan2(a).to_degrees();
    let rotation = if rotation <= -180.0 + 1e-9 { 180.0 } else { rotation };
    // A flattened transform (e.g. scale_y = 0, so `det` and the shear numerator are both 0) has
    // no recoverable shear: 0 / 0 would be NaN, and a NaN here poisons the object when the
    // caller feeds the decomposed values back into `compose` (transform.set does exactly that).
    // Report 0 instead; a non-zero numerator over a zero determinant still limits to ±90°.
    let shear = ((a * c + b * d) / det).atan().to_degrees();
    let shear = if shear.is_nan() { 0.0 } else { shear };
    Decomposed { scale_x: sx, scale_y: det / sx, rotation, shear }
}

/// The linear transform with these values (no translation); `decompose` inverts it.
pub fn compose(v: &Decomposed) -> Affine {
    Affine::rotate(-v.rotation.to_radians())
        * Affine::new([1.0, 0.0, v.shear.to_radians().tan(), 1.0, 0.0, 0.0])
        * Affine::scale_non_uniform(v.scale_x, v.scale_y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        for (sx, sy, r, k) in [(1.0, 1.0, 0.0, 0.0), (2.0, 0.5, 30.0, 0.0), (1.5, -1.0, -120.0, 20.0), (0.25, 3.0, 180.0, -45.0)] {
            let v = Decomposed { scale_x: sx, scale_y: sy, rotation: r, shear: k };
            let back = decompose(compose(&v));
            for (x, y) in [(back.scale_x, sx), (back.scale_y, sy), (back.rotation, r), (back.shear, k)] {
                assert!((x - y).abs() < 1e-9, "{v:?} → {back:?}");
            }
        }
        // transform.rotate's convention: rotate(-θ) is θ counter-clockwise.
        assert!((decompose(Affine::rotate(-0.5)).rotation - 0.5f64.to_degrees()).abs() < 1e-9);
    }

    #[test]
    fn flattened_transform_has_no_nan_shear() {
        // Regression: an object scaled to zero height (scale_y = 0) is singular — its
        // determinant and shear numerator are both 0, and decompose used to report NaN shear,
        // which compose then turned into a NaN transform. Shear is unrecoverable there, so
        // it reports 0 and every other field stays finite and exact.
        for v in [
            Decomposed { scale_x: 1.0, scale_y: 0.0, rotation: 0.0, shear: 20.0 },
            Decomposed { scale_x: 2.0, scale_y: 0.0, rotation: 30.0, shear: -45.0 },
        ] {
            let back = decompose(compose(&v));
            assert!(back.scale_x.is_finite() && back.scale_y.is_finite() && back.rotation.is_finite() && back.shear.is_finite(), "{v:?} → {back:?}");
            assert_eq!(back.shear, 0.0, "{v:?} → {back:?}");
            assert!((back.scale_x - v.scale_x).abs() < 1e-9, "{v:?} → {back:?}");
            assert_eq!(back.scale_y, 0.0, "{v:?} → {back:?}");
            assert!((back.rotation - v.rotation).abs() < 1e-9, "{v:?} → {back:?}");
            // Feeding the decomposed values back into compose stays finite too.
            assert!(compose(&back).as_coeffs().iter().all(|c| c.is_finite()), "{v:?} → {back:?}");
        }
        // Directly singular matrices, including one whose shear numerator is non-zero
        // (that case limits to ±90°, not NaN).
        assert_eq!(decompose(Affine::new([1.0, 0.0, 0.0, 0.0, 0.0, 0.0])).shear, 0.0);
        assert!((decompose(Affine::new([1.0, 0.0, 1.0, 0.0, 0.0, 0.0])).shear.abs() - 90.0).abs() < 1e-9);
    }
}
