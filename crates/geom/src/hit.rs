//! Hit testing primitives.

use kurbo::{BezPath, ParamCurveNearest, Point, Rect, Shape};

use crate::path::{FillRule, PathData};

/// Is `p` inside the filled area of `path` under `rule`? Open subpaths are implicitly closed (as when filled).
pub fn fill_contains(path: &BezPath, rule: FillRule, p: Point) -> bool {
    let w = path.winding(p);
    match rule {
        FillRule::NonZero => w != 0,
        FillRule::EvenOdd => w % 2 != 0,
    }
}

/// Distance from `p` to the nearest point on the path outline.
pub fn distance_to_outline(path: &BezPath, p: Point) -> f64 {
    path.segments().map(|s| s.nearest(p, 1e-6).distance_sq).fold(f64::INFINITY, f64::min).sqrt()
}

/// Is `p` within `tol` of the path's stroke of width `width`?
pub fn stroke_contains(path: &BezPath, width: f64, tol: f64, p: Point) -> bool {
    let quick = path.bounding_box().inflate(width / 2.0 + tol, width / 2.0 + tol);
    quick.contains(p) && distance_to_outline(path, p) <= width / 2.0 + tol
}

/// Does the path intersect (or lie inside) the rect? Used for marquee selection.
pub fn intersects_rect(path: &PathData, r: Rect) -> bool {
    let Some(b) = path.bounds() else { return false };
    let flat = |q: Rect| q.width() == 0.0 || q.height() == 0.0;
    if b.intersect(r).area() <= 0.0 && !flat(b) && !flat(r) {
        return false;
    }
    // Any anchor inside?
    if path.anchors().any(|(_, _, a)| r.contains(a.p)) {
        return true;
    }
    // Any segment crossing the rect edges?
    let edges = [
        kurbo::Line::new((r.x0, r.y0), (r.x1, r.y0)),
        kurbo::Line::new((r.x1, r.y0), (r.x1, r.y1)),
        kurbo::Line::new((r.x1, r.y1), (r.x0, r.y1)),
        kurbo::Line::new((r.x0, r.y1), (r.x0, r.y0)),
    ];
    let bp = path.to_bezpath();
    for seg in bp.segments() {
        for e in &edges {
            if !seg.intersect_line(*e).is_empty() {
                return true;
            }
        }
    }
    // Rect entirely inside a closed shape counts as touching it.
    path.is_closed() && fill_contains(&bp, FillRule::NonZero, r.center())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shapes;

    #[test]
    fn fill_and_stroke() {
        let p = shapes::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0)).to_bezpath();
        assert!(fill_contains(&p, FillRule::NonZero, Point::new(5.0, 5.0)));
        assert!(!fill_contains(&p, FillRule::NonZero, Point::new(15.0, 5.0)));
        assert!(stroke_contains(&p, 2.0, 0.0, Point::new(10.5, 5.0)));
        assert!(!stroke_contains(&p, 2.0, 0.0, Point::new(12.0, 5.0)));
    }

    #[test]
    fn evenodd_hole() {
        let mut outer = shapes::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0));
        outer.subpaths.extend(shapes::rectangle(Rect::new(3.0, 3.0, 7.0, 7.0)).subpaths);
        let bp = outer.to_bezpath();
        assert!(!fill_contains(&bp, FillRule::EvenOdd, Point::new(5.0, 5.0)));
        assert!(fill_contains(&bp, FillRule::NonZero, Point::new(5.0, 5.0)));
        assert!(fill_contains(&bp, FillRule::EvenOdd, Point::new(1.0, 5.0)));
    }

    #[test]
    fn marquee() {
        let p = shapes::ellipse(Rect::new(0.0, 0.0, 10.0, 10.0));
        assert!(intersects_rect(&p, Rect::new(-1.0, 4.0, 1.0, 6.0)));
        assert!(!intersects_rect(&p, Rect::new(20.0, 20.0, 30.0, 30.0)));
        // corner region of the bbox that misses the ellipse outline
        assert!(!intersects_rect(&p, Rect::new(0.0, 0.0, 0.5, 0.5)));
        // fully inside
        assert!(intersects_rect(&p, Rect::new(4.0, 4.0, 6.0, 6.0)));
        // crossing a line
        let l = shapes::line(Point::new(0.0, 0.0), Point::new(10.0, 10.0));
        assert!(intersects_rect(&l, Rect::new(4.0, 0.0, 6.0, 10.0)));
    }

    #[test]
    fn flat_marquee() {
        let p = shapes::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0));
        assert!(intersects_rect(&p, Rect::new(-5.0, 5.0, 15.0, 5.0)));
        assert!(intersects_rect(&p, Rect::new(5.0, -5.0, 5.0, 15.0)));
        assert!(!intersects_rect(&p, Rect::new(-5.0, 20.0, 15.0, 20.0)));
    }
}
