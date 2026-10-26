//! The anchor-based editable path model.

use kurbo::{Affine, BezPath, CubicBez, ParamCurve, ParamCurveNearest, PathEl, Point, Rect, Shape};
use serde::{Deserialize, Serialize};

use crate::EPS;

/// Fill rule for paths and compound paths.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FillRule {
    /// InDesign's default for compound paths created from overlapping shapes.
    #[default]
    NonZero,
    EvenOdd,
}

/// How an anchor's handles behave when edited.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AnchorKind {
    /// Handles move independently (or there are none).
    #[default]
    Corner,
    /// Handles stay collinear (dragging one rotates the other).
    Smooth,
}

/// One anchor point with absolute handle positions. A handle equal to `p` means "no handle".
/// Serialized without handles that equal `p` and without the default `kind` (smaller files).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "AnchorRepr", into = "AnchorRepr")]
pub struct Anchor {
    pub p: Point,
    #[serde(rename = "in")]
    pub h_in: Point,
    #[serde(rename = "out")]
    pub h_out: Point,
    #[serde(default)]
    pub kind: AnchorKind,
}

/// Wire form of [`Anchor`]: points as `[x, y]` (format v2; `{"x", "y"}` maps from v1 files are
/// still read), and absent handles mean "no handle" (equal to `p`).
#[derive(Serialize, Deserialize)]
struct AnchorRepr {
    p: WirePoint,
    #[serde(rename = "in", default, skip_serializing_if = "Option::is_none")]
    h_in: Option<WirePoint>,
    #[serde(rename = "out", default, skip_serializing_if = "Option::is_none")]
    h_out: Option<WirePoint>,
    #[serde(default, skip_serializing_if = "is_corner")]
    kind: AnchorKind,
}

fn is_corner(k: &AnchorKind) -> bool {
    *k == AnchorKind::Corner
}

impl From<AnchorRepr> for Anchor {
    fn from(r: AnchorRepr) -> Self {
        let p = r.p.0;
        Self { p, h_in: r.h_in.map_or(p, |h| h.0), h_out: r.h_out.map_or(p, |h| h.0), kind: r.kind }
    }
}

impl From<Anchor> for AnchorRepr {
    fn from(a: Anchor) -> Self {
        let handle = |h: Point| (h != a.p).then_some(WirePoint(h));
        Self { p: WirePoint(a.p), h_in: handle(a.h_in), h_out: handle(a.h_out), kind: a.kind }
    }
}

/// A point written as `[x, y]`, read from `[x, y]` or `{"x": …, "y": …}`.
struct WirePoint(Point);

impl Serialize for WirePoint {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        [self.0.x, self.0.y].serialize(s)
    }
}

impl<'de> Deserialize<'de> for WirePoint {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = WirePoint;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a point as [x, y] or {\"x\": x, \"y\": y}")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut a: A) -> Result<WirePoint, A::Error> {
                let x = a.next_element()?.ok_or_else(|| serde::de::Error::invalid_length(0, &self))?;
                let y = a.next_element()?.ok_or_else(|| serde::de::Error::invalid_length(1, &self))?;
                Ok(WirePoint(Point::new(x, y)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut a: A) -> Result<WirePoint, A::Error> {
                let (mut x, mut y) = (None, None);
                while let Some(k) = a.next_key::<std::borrow::Cow<'de, str>>()? {
                    match k.as_ref() {
                        "x" => x = Some(a.next_value()?),
                        "y" => y = Some(a.next_value()?),
                        _ => {
                            a.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(WirePoint(Point::new(
                    x.ok_or_else(|| serde::de::Error::missing_field("x"))?,
                    y.ok_or_else(|| serde::de::Error::missing_field("y"))?,
                )))
            }
        }
        d.deserialize_any(V)
    }
}

impl Anchor {
    /// A corner anchor without handles.
    pub fn corner(p: Point) -> Self {
        Self { p, h_in: p, h_out: p, kind: AnchorKind::Corner }
    }
    /// A smooth anchor with a symmetric out handle `out` (the in handle is mirrored).
    pub fn smooth(p: Point, out: Point) -> Self {
        Self { p, h_in: p - (out - p), h_out: out, kind: AnchorKind::Smooth }
    }
    pub fn with_handles(p: Point, h_in: Point, h_out: Point) -> Self {
        let kind =
            if is_collinear(p, h_in, h_out) && h_in.distance(p) > EPS && h_out.distance(p) > EPS { AnchorKind::Smooth } else { AnchorKind::Corner };
        Self { p, h_in, h_out, kind }
    }
    pub fn has_in(&self) -> bool {
        self.h_in.distance(self.p) > EPS
    }
    pub fn has_out(&self) -> bool {
        self.h_out.distance(self.p) > EPS
    }
    pub fn transform(&self, a: Affine) -> Self {
        Self { p: a * self.p, h_in: a * self.h_in, h_out: a * self.h_out, kind: self.kind }
    }
    pub fn translate(&mut self, d: kurbo::Vec2) {
        self.p += d;
        self.h_in += d;
        self.h_out += d;
    }
    /// Swap handles (used when reversing path direction).
    pub fn reversed(&self) -> Self {
        Self { p: self.p, h_in: self.h_out, h_out: self.h_in, kind: self.kind }
    }
    /// Remove both handles (convert to a sharp corner).
    pub fn retract(&mut self) {
        self.h_in = self.p;
        self.h_out = self.p;
        self.kind = AnchorKind::Corner;
    }
}

fn is_collinear(p: Point, a: Point, b: Point) -> bool {
    let va = a - p;
    let vb = b - p;
    let cross = va.cross(vb);
    cross.abs() <= 1e-6 * va.hypot().max(1.0) * vb.hypot().max(1.0) && va.dot(vb) < 0.0
}

/// An open or closed run of anchors.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SubPath {
    pub anchors: Vec<Anchor>,
    #[serde(default)]
    pub closed: bool,
}

impl SubPath {
    pub fn new(anchors: Vec<Anchor>, closed: bool) -> Self {
        Self { anchors, closed }
    }
    /// Polyline from points.
    pub fn polyline(points: &[Point], closed: bool) -> Self {
        Self { anchors: points.iter().map(|&p| Anchor::corner(p)).collect(), closed }
    }
    /// Number of segments (a closed path has one more, back to the start).
    pub fn segment_count(&self) -> usize {
        let n = self.anchors.len();
        if n < 2 {
            0
        } else if self.closed {
            n
        } else {
            n - 1
        }
    }
    /// Segment `i` as a cubic (lines are cubics with handles on the anchors).
    pub fn segment(&self, i: usize) -> CubicBez {
        let n = self.anchors.len();
        let a = &self.anchors[i % n];
        let b = &self.anchors[(i + 1) % n];
        CubicBez::new(a.p, a.h_out, b.h_in, b.p)
    }
    /// True if segment `i` is a straight line.
    pub fn segment_is_line(&self, i: usize) -> bool {
        let n = self.anchors.len();
        let a = &self.anchors[i % n];
        let b = &self.anchors[(i + 1) % n];
        !a.has_out() && !b.has_in()
    }
    pub fn to_bezpath_into(&self, out: &mut BezPath) {
        let Some(first) = self.anchors.first() else { return };
        out.move_to(first.p);
        for i in 0..self.segment_count() {
            let c = self.segment(i);
            if self.segment_is_line(i) {
                out.line_to(c.p3);
            } else {
                out.curve_to(c.p1, c.p2, c.p3);
            }
        }
        if self.closed {
            out.close_path();
        }
    }
    pub fn reverse(&mut self) {
        self.anchors.reverse();
        for a in &mut self.anchors {
            *a = a.reversed();
        }
    }
    /// Split segment `seg` at parameter `t`, inserting a new anchor. Returns the new anchor index.
    pub fn insert_anchor(&mut self, seg: usize, t: f64) -> usize {
        let n = self.anchors.len();
        let c = self.segment(seg);
        let line = self.segment_is_line(seg);
        let (l, r) = (c.subsegment(0.0..t), c.subsegment(t..1.0));
        let i0 = seg % n;
        let i1 = (seg + 1) % n;
        let mid = if line {
            Anchor::corner(l.p3)
        } else {
            self.anchors[i0].h_out = l.p1;
            self.anchors[i1].h_in = r.p2;
            Anchor { p: l.p3, h_in: l.p2, h_out: r.p1, kind: AnchorKind::Smooth }
        };
        self.anchors.insert(seg + 1, mid);
        seg + 1
    }
    /// Signed area via the shoelace formula on the Bézier path (positive = clockwise in y-down).
    pub fn area(&self) -> f64 {
        let mut bp = BezPath::new();
        self.to_bezpath_into(&mut bp);
        bp.area()
    }
}

/// A (possibly multi-subpath) path.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PathData {
    pub subpaths: Vec<SubPath>,
}

impl PathData {
    pub fn new(subpaths: Vec<SubPath>) -> Self {
        Self { subpaths }
    }
    pub fn single(sp: SubPath) -> Self {
        Self { subpaths: vec![sp] }
    }
    pub fn is_empty(&self) -> bool {
        self.subpaths.iter().all(|s| s.anchors.is_empty())
    }
    pub fn anchor_count(&self) -> usize {
        self.subpaths.iter().map(|s| s.anchors.len()).sum()
    }
    pub fn to_bezpath(&self) -> BezPath {
        let mut bp = BezPath::new();
        for sp in &self.subpaths {
            sp.to_bezpath_into(&mut bp);
        }
        bp
    }
    /// Convert a kurbo path to anchors. Quadratics are elevated to cubics.
    pub fn from_bezpath(bp: &BezPath) -> Self {
        let mut subpaths: Vec<SubPath> = Vec::new();
        let mut cur: Option<SubPath> = None;
        let mut last = Point::ZERO;
        for el in bp.elements() {
            match *el {
                PathEl::MoveTo(p) => {
                    if let Some(mut sp) = cur.take()
                        && !sp.anchors.is_empty()
                    {
                        for a in &mut sp.anchors {
                            *a = Anchor::with_handles(a.p, a.h_in, a.h_out);
                        }
                        subpaths.push(sp);
                    }
                    cur = Some(SubPath { anchors: vec![Anchor::corner(p)], closed: false });
                    last = p;
                }
                PathEl::LineTo(p) => {
                    let sp = cur.get_or_insert_with(|| SubPath { anchors: vec![Anchor::corner(last)], closed: false });
                    sp.anchors.push(Anchor::corner(p));
                    last = p;
                }
                PathEl::QuadTo(q, p) => {
                    let c1 = last + (q - last) * (2.0 / 3.0);
                    let c2 = p + (q - p) * (2.0 / 3.0);
                    push_curve(&mut cur, last, c1, c2, p);
                    last = p;
                }
                PathEl::CurveTo(c1, c2, p) => {
                    push_curve(&mut cur, last, c1, c2, p);
                    last = p;
                }
                PathEl::ClosePath => {
                    if let Some(mut sp) = cur.take() {
                        // Merge a closing anchor that duplicates the start.
                        if sp.anchors.len() > 1
                            && let (Some(first), Some(last)) = (sp.anchors.first(), sp.anchors.last())
                            && first.p.distance(last.p) < 1e-7
                            && let Some(l) = sp.anchors.pop()
                        {
                            sp.anchors[0].h_in = l.h_in;
                        }
                        sp.closed = true;
                        for a in &mut sp.anchors {
                            *a = Anchor::with_handles(a.p, a.h_in, a.h_out);
                        }
                        last = sp.anchors.first().map(|a| a.p).unwrap_or(last);
                        subpaths.push(sp);
                    }
                }
            }
        }
        if let Some(mut sp) = cur
            && !sp.anchors.is_empty()
        {
            for a in &mut sp.anchors {
                *a = Anchor::with_handles(a.p, a.h_in, a.h_out);
            }
            subpaths.push(sp);
        }
        Self { subpaths }
    }
    /// Tight geometric bounds (curve extrema, not control points).
    pub fn bounds(&self) -> Option<Rect> {
        if self.is_empty() {
            return None;
        }
        let bp = self.to_bezpath();
        if self.anchor_count() == 1
            && let Some(a) = self.subpaths.iter().find_map(|s| s.anchors.first())
        {
            return Some(Rect::from_points(a.p, a.p));
        }
        Some(bp.bounding_box())
    }
    /// Bounds including control handles (used for quick culling).
    pub fn control_bounds(&self) -> Option<Rect> {
        let mut it = self.subpaths.iter().flat_map(|s| s.anchors.iter()).flat_map(|a| [a.p, a.h_in, a.h_out]);
        let first = it.next()?;
        Some(it.fold(Rect::from_points(first, first), |r, p| r.union_pt(p)))
    }
    pub fn transform(&mut self, a: Affine) {
        for sp in &mut self.subpaths {
            for an in &mut sp.anchors {
                *an = an.transform(a);
            }
        }
    }
    pub fn transformed(&self, a: Affine) -> Self {
        let mut c = self.clone();
        c.transform(a);
        c
    }
    pub fn is_closed(&self) -> bool {
        !self.subpaths.is_empty() && self.subpaths.iter().all(|s| s.closed)
    }
    /// Nearest point on any segment: (subpath, segment, t, point, distance).
    pub fn nearest(&self, p: Point) -> Option<(usize, usize, f64, Point, f64)> {
        let mut best: Option<(usize, usize, f64, Point, f64)> = None;
        for (si, sp) in self.subpaths.iter().enumerate() {
            for seg in 0..sp.segment_count() {
                let c = sp.segment(seg);
                let n = c.nearest(p, 1e-6);
                let q = c.eval(n.t);
                let d = n.distance_sq.sqrt();
                if best.is_none_or(|b| d < b.4) {
                    best = Some((si, seg, n.t, q, d));
                }
            }
        }
        best
    }
    /// Iterate (subpath index, anchor index, anchor).
    pub fn anchors(&self) -> impl Iterator<Item = (usize, usize, &Anchor)> {
        self.subpaths.iter().enumerate().flat_map(|(si, s)| s.anchors.iter().enumerate().map(move |(ai, a)| (si, ai, a)))
    }
    pub fn anchor_mut(&mut self, si: usize, ai: usize) -> Option<&mut Anchor> {
        self.subpaths.get_mut(si)?.anchors.get_mut(ai)
    }
    pub fn reverse(&mut self) {
        for sp in &mut self.subpaths {
            sp.reverse();
        }
    }
    /// Total length of all segments.
    pub fn length(&self) -> f64 {
        self.to_bezpath().segments().map(|s| kurbo::ParamCurveArclen::arclen(&s, 1e-6)).sum()
    }
}

fn push_curve(cur: &mut Option<SubPath>, last: Point, c1: Point, c2: Point, p: Point) {
    let sp = cur.get_or_insert_with(|| SubPath { anchors: vec![Anchor::corner(last)], closed: false });
    if let Some(a) = sp.anchors.last_mut() {
        a.h_out = c1;
    }
    sp.anchors.push(Anchor { p, h_in: c2, h_out: p, kind: AnchorKind::Corner });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> PathData {
        PathData::single(SubPath::polyline(&[Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 10.0), Point::new(0.0, 10.0)], true))
    }

    #[test]
    fn from_bezpath_detects_smooth_on_every_open_subpath() {
        let mut bp = BezPath::new();
        for _ in 0..2 {
            bp.move_to((0.0, 0.0));
            bp.curve_to((0.0, 10.0), (10.0, 10.0), (10.0, 0.0));
            bp.curve_to((10.0, -10.0), (20.0, -10.0), (20.0, 0.0));
        }
        let pd = PathData::from_bezpath(&bp);
        assert_eq!(pd.subpaths.len(), 2);
        assert_eq!(pd.subpaths[1].anchors[1].kind, AnchorKind::Smooth);
        assert_eq!(pd.subpaths[0].anchors[1].kind, pd.subpaths[1].anchors[1].kind);
    }

    #[test]
    fn anchors_serialize_compactly_and_read_v1_maps() {
        let corner = Anchor::corner(Point::new(1.0, 2.0));
        assert_eq!(serde_json::to_string(&corner).unwrap(), r#"{"p":[1.0,2.0]}"#);
        let smooth = Anchor { p: Point::new(1.0, 2.0), h_in: Point::new(0.0, 2.0), h_out: Point::new(2.0, 2.0), kind: AnchorKind::Smooth };
        let back: Anchor = serde_json::from_str(&serde_json::to_string(&smooth).unwrap()).unwrap();
        assert_eq!(back, smooth);
        let v1 = r#"{"p":{"x":1.0,"y":2.0},"in":{"x":0.0,"y":2.0},"out":{"x":2.0,"y":2.0},"kind":"Smooth"}"#;
        assert_eq!(serde_json::from_str::<Anchor>(v1).unwrap(), smooth);
        let v1_corner = r#"{"p":{"x":1.0,"y":2.0},"in":{"x":1.0,"y":2.0},"out":{"x":1.0,"y":2.0},"kind":"Corner"}"#;
        assert_eq!(serde_json::from_str::<Anchor>(v1_corner).unwrap(), corner);
    }

    #[test]
    fn square_bounds_and_area() {
        let p = square();
        assert_eq!(p.bounds(), Some(Rect::new(0.0, 0.0, 10.0, 10.0)));
        assert!((p.subpaths[0].area().abs() - 100.0).abs() < 1e-9);
        assert_eq!(p.subpaths[0].segment_count(), 4);
    }

    #[test]
    fn bezpath_roundtrip_polyline() {
        let p = square();
        let bp = p.to_bezpath();
        let back = PathData::from_bezpath(&bp);
        assert_eq!(back.subpaths.len(), 1);
        assert_eq!(back.subpaths[0].anchors.len(), 4);
        assert!(back.subpaths[0].closed);
        assert_eq!(back.bounds(), p.bounds());
    }

    #[test]
    fn bezpath_roundtrip_curves() {
        let c = kurbo::Circle::new((50.0, 50.0), 20.0);
        let bp = c.to_path(1e-3);
        let pd = PathData::from_bezpath(&bp);
        assert_eq!(pd.subpaths.len(), 1);
        assert!(pd.subpaths[0].closed);
        let b = pd.bounds().unwrap();
        assert!((b.width() - 40.0).abs() < 1e-3);
        // All anchors of a circle are smooth.
        assert!(pd.subpaths[0].anchors.iter().all(|a| a.kind == AnchorKind::Smooth));
    }

    #[test]
    fn insert_anchor_on_line_keeps_shape() {
        let mut p = square();
        let before = p.bounds();
        let idx = p.subpaths[0].insert_anchor(0, 0.5);
        assert_eq!(idx, 1);
        assert_eq!(p.subpaths[0].anchors[1].p, Point::new(5.0, 0.0));
        assert_eq!(p.bounds(), before);
        assert_eq!(p.anchor_count(), 5);
    }

    #[test]
    fn insert_anchor_on_curve_keeps_shape() {
        let bp = kurbo::Circle::new((0.0, 0.0), 10.0).to_path(1e-3);
        let mut pd = PathData::from_bezpath(&bp);
        let area0 = pd.subpaths[0].area();
        pd.subpaths[0].insert_anchor(1, 0.3);
        assert!((pd.subpaths[0].area() - area0).abs() < 1e-6);
    }

    #[test]
    fn nearest_on_edge() {
        let p = square();
        let (_, seg, _, q, d) = p.nearest(Point::new(5.0, -3.0)).unwrap();
        assert_eq!(seg, 0);
        assert!((q.x - 5.0).abs() < 1e-6 && q.y.abs() < 1e-6);
        assert!((d - 3.0).abs() < 1e-6);
    }

    #[test]
    fn reverse_flips_area_sign() {
        let mut p = square();
        let a = p.subpaths[0].area();
        p.reverse();
        assert!((p.subpaths[0].area() + a).abs() < 1e-9);
    }

    #[test]
    fn transform_moves_bounds() {
        let p = square().transformed(Affine::translate((5.0, 5.0)));
        assert_eq!(p.bounds(), Some(Rect::new(5.0, 5.0, 15.0, 15.0)));
    }

    #[test]
    fn smooth_detection() {
        let a = Anchor::with_handles(Point::new(0.0, 0.0), Point::new(-1.0, 0.0), Point::new(2.0, 0.0));
        assert_eq!(a.kind, AnchorKind::Smooth);
        let c = Anchor::with_handles(Point::new(0.0, 0.0), Point::new(-1.0, 0.0), Point::new(0.0, 2.0));
        assert_eq!(c.kind, AnchorKind::Corner);
    }

    #[test]
    fn length_of_square() {
        assert!((square().length() - 40.0).abs() < 1e-6);
    }

    #[test]
    fn serde_roundtrip() {
        let p = square();
        let s = serde_json::to_string(&p).unwrap();
        let back: PathData = serde_json::from_str(&s).unwrap();
        assert_eq!(p, back);
    }
}
