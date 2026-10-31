//! DesignCraft geometry kernel.
//!
//! The editable path model is anchor based (like every InDesign tool thinks about paths):
//! a [`PathData`] is a list of [`SubPath`]s, each a list of [`Anchor`]s with absolute in/out
//! handle positions. Rendering and algorithms convert to [`kurbo::BezPath`].
//!
//! Coordinates are document points (1/72 in), y pointing down.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod corners;
pub mod decompose;
pub mod edit_path;
pub mod freehand;
pub mod hit;
pub mod path;
pub mod pathfinder;
pub mod qr;
pub mod shapes;
pub mod snap;
pub mod stroke_style;
pub mod trace;
pub mod units;
pub mod warp;

pub use kurbo;
pub use kurbo::{Affine, BezPath, CubicBez, Line, ParamCurve, PathEl, PathSeg, Point, Rect, Shape, Size, Vec2};
pub use path::{Anchor, AnchorKind, FillRule, PathData, SubPath};
pub use units::{Unit, decimal_comma, format_measure, parse_measure};

/// Tolerance used when comparing handle positions to anchor positions.
pub const EPS: f64 = 1e-9;

/// Union of two optional rects.
pub fn union_opt(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.union(b)),
        (a, None) => a,
        (None, b) => b,
    }
}

/// Normalize an angle in degrees to (-180, 180].
pub fn normalize_deg(mut a: f64) -> f64 {
    a %= 360.0;
    if a <= -180.0 {
        a += 360.0;
    } else if a > 180.0 {
        a -= 360.0;
    }
    a
}

/// Snap a vector to the nearest multiple of `step_deg` (used for Shift-constrained drags).
pub fn constrain_angle(v: Vec2, step_deg: f64) -> Vec2 {
    let len = v.hypot();
    if len < EPS {
        return v;
    }
    let step = step_deg.to_radians();
    let a = (v.y.atan2(v.x) / step).round() * step;
    Vec2::new(a.cos() * len, a.sin() * len)
}

/// Rect from two corner points in any order.
pub fn rect_from_points(a: Point, b: Point) -> Rect {
    Rect::from_points(a, b)
}

/// The 9 reference points of a rect (InDesign's reference-point locator), row-major from top-left.
pub fn reference_point(r: Rect, index: usize) -> Point {
    let xs = [r.x0, r.center().x, r.x1];
    let ys = [r.y0, r.center().y, r.y1];
    Point::new(xs[index % 3], ys[(index / 3).min(2)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angle_normalization() {
        assert_eq!(normalize_deg(190.0), -170.0);
        assert_eq!(normalize_deg(-190.0), 170.0);
        assert_eq!(normalize_deg(180.0), 180.0);
        assert_eq!(normalize_deg(720.0), 0.0);
    }

    #[test]
    fn constrain_45() {
        let v = constrain_angle(Vec2::new(10.0, 1.0), 45.0);
        assert!((v.y).abs() < 1e-9);
        let v = constrain_angle(Vec2::new(10.0, 9.0), 45.0);
        assert!((v.x - v.y).abs() < 1e-9);
    }

    #[test]
    fn reference_points() {
        let r = Rect::new(0.0, 0.0, 10.0, 20.0);
        assert_eq!(reference_point(r, 0), Point::new(0.0, 0.0));
        assert_eq!(reference_point(r, 4), Point::new(5.0, 10.0));
        assert_eq!(reference_point(r, 8), Point::new(10.0, 20.0));
    }

    #[test]
    fn union_options() {
        let a = Rect::new(0.0, 0.0, 1.0, 1.0);
        let b = Rect::new(2.0, 2.0, 3.0, 3.0);
        assert_eq!(union_opt(Some(a), Some(b)), Some(Rect::new(0.0, 0.0, 3.0, 3.0)));
        assert_eq!(union_opt(None, Some(b)), Some(b));
        assert_eq!(union_opt(None, None), None);
    }
}
