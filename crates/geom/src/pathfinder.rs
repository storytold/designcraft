//! Pathfinder: Add, Subtract, Intersect, Exclude Overlap and Minus Back on closed paths, keeping
//! curves as curves (flo_curves' Bézier path arithmetic).

use flo_curves::bezier::path::{SimpleBezierPath, path_add, path_intersect, path_sub};
use flo_curves::geo::Coord2;

use crate::Point;
use crate::path::{Anchor, AnchorKind, PathData, SubPath};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// Union of all shapes.
    Add,
    /// The front shapes cut out of the backmost one.
    Subtract,
    /// The area all shapes share.
    Intersect,
    /// Areas covered an odd number of times.
    Exclude,
    /// The back shapes cut out of the frontmost one.
    MinusBack,
}

impl Op {
    pub fn parse(s: &str) -> Option<Op> {
        Some(match s {
            "add" => Op::Add,
            "subtract" => Op::Subtract,
            "intersect" => Op::Intersect,
            "exclude" => Op::Exclude,
            "minusBack" => Op::MinusBack,
            _ => return None,
        })
    }
}

const ACCURACY: f64 = 0.01;

fn c(p: Point) -> Coord2 {
    Coord2(p.x, p.y)
}

/// Every subpath as a closed Bézier path (open ones are closed with a straight segment).
fn to_flo(path: &PathData) -> Vec<SimpleBezierPath> {
    let mut out = Vec::new();
    for sp in &path.subpaths {
        if sp.anchors.len() < 2 {
            continue;
        }
        let mut closed = sp.clone();
        closed.closed = true;
        let curves = (0..closed.segment_count()).map(|i| {
            let s = closed.segment(i);
            (c(s.p1), c(s.p2), c(s.p3))
        });
        out.push((c(closed.anchors[0].p), curves.collect()));
    }
    out
}

fn from_flo(paths: &[SimpleBezierPath]) -> PathData {
    let p = |c: &Coord2| Point::new(c.0, c.1);
    let mut subpaths = Vec::new();
    for (start, curves) in paths {
        if curves.is_empty() {
            continue;
        }
        let mut anchors = vec![Anchor::corner(p(start))];
        for (c1, c2, end) in curves {
            if let Some(prev) = anchors.last_mut() {
                prev.h_out = p(c1);
            }
            let mut a = Anchor::corner(p(end));
            a.h_in = p(c2);
            anchors.push(a);
        }
        // The last curve comes back to the start: fold its end into the first anchor.
        if anchors.len() > 2
            && anchors.last().is_some_and(|l| l.p.distance(anchors[0].p) < 1e-6)
            && let Some(last) = anchors.pop()
        {
            anchors[0].h_in = last.h_in;
        }
        for a in &mut anchors {
            *a = Anchor::with_handles(a.p, a.h_in, a.h_out);
            if !a.has_in() && !a.has_out() {
                a.kind = AnchorKind::Corner;
            }
        }
        subpaths.push(SubPath::new(anchors, true));
    }
    PathData::new(subpaths)
}

/// Unions of more shapes than this combine in pairs; up to it each is added in turn (as always).
const BALANCED_UNION_FROM: usize = 16;

/// Combine `shapes` (back to front, all in one coordinate space). `None` when nothing is left.
pub fn combine(op: Op, shapes: &[PathData]) -> Option<PathData> {
    let all: Vec<Vec<SimpleBezierPath>> = shapes.iter().map(to_flo).collect();
    // The base shape is chosen by position; a degenerate base leaves nothing.
    let base = match op {
        Op::Subtract => all.first(),
        Op::MinusBack => all.last(),
        _ => None,
    };
    if base.is_some_and(|b| b.is_empty()) {
        return None;
    }
    // A shape with no area shares none with the others: Intersect leaves nothing.
    if op == Op::Intersect && all.iter().any(|p| p.is_empty()) {
        return None;
    }
    let flo: Vec<Vec<SimpleBezierPath>> = all.into_iter().filter(|p| !p.is_empty()).collect();
    let (first, rest) = flo.split_first()?;
    let union = |v: &[Vec<SimpleBezierPath>]| -> Vec<SimpleBezierPath> {
        if v.len() > BALANCED_UNION_FROM {
            // Pairs, then pairs of pairs: adding each shape to everything so far reprocesses the
            // growing result every time (quadratic for many shapes).
            let mut level: Vec<Vec<SimpleBezierPath>> = v.to_vec();
            while level.len() > 1 {
                level = level
                    .chunks(2)
                    .map(|c| match c {
                        [a, b] if a.is_empty() => b.clone(),
                        [a, b] if b.is_empty() => a.clone(),
                        [a, b] => path_add(a, b, ACCURACY),
                        _ => c.first().cloned().unwrap_or_default(),
                    })
                    .collect();
            }
            return level.pop().unwrap_or_default();
        }
        let mut acc: Vec<SimpleBezierPath> = Vec::new();
        for p in v {
            acc = if acc.is_empty() { p.clone() } else { path_add(&acc, p, ACCURACY) };
        }
        acc
    };
    let out: Vec<SimpleBezierPath> = match op {
        Op::Add => union(&flo),
        Op::Subtract => {
            let cut = union(rest);
            if cut.is_empty() { first.clone() } else { path_sub(first, &cut, ACCURACY) }
        }
        Op::MinusBack => {
            let (front, backs) = flo.split_last()?;
            let cut = union(backs);
            if cut.is_empty() { front.clone() } else { path_sub(front, &cut, ACCURACY) }
        }
        Op::Intersect => {
            let mut acc = first.clone();
            for p in rest {
                acc = path_intersect(&acc, p, ACCURACY);
                if acc.is_empty() {
                    break;
                }
            }
            acc
        }
        Op::Exclude => {
            let mut acc = first.clone();
            for p in rest {
                let a: Vec<SimpleBezierPath> = path_sub(&acc, p, ACCURACY);
                let b: Vec<SimpleBezierPath> = path_sub(p, &acc, ACCURACY);
                acc = a.into_iter().chain(b).collect();
            }
            acc
        }
    };
    let r = from_flo(&out);
    if r.subpaths.is_empty() { None } else { Some(r) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Rect, shapes};

    fn area(p: &PathData) -> f64 {
        p.subpaths.iter().map(|s| s.area()).sum::<f64>().abs()
    }

    #[test]
    fn a_union_of_many_shapes_adds_up() {
        // 40 squares in a row, each overlapping the next by 5: width 40 * 10 + 5, height 10.
        let shapes: Vec<PathData> = (0..40).map(|i| shapes::rectangle(Rect::new(f64::from(i) * 5.0, 0.0, f64::from(i) * 5.0 + 10.0, 10.0))).collect();
        let u = combine(Op::Add, &shapes).unwrap();
        assert!((area(&u) - 205.0 * 10.0).abs() < 1.0, "{}", area(&u));
        // Far-apart squares stay separate.
        let apart: Vec<PathData> =
            (0..40).map(|i| shapes::rectangle(Rect::new(f64::from(i) * 50.0, 0.0, f64::from(i) * 50.0 + 10.0, 10.0))).collect();
        let u = combine(Op::Add, &apart).unwrap();
        assert_eq!(u.subpaths.len(), 40);
    }

    #[test]
    fn boolean_ops_on_overlapping_squares() {
        let a = shapes::rectangle(Rect::new(0.0, 0.0, 100.0, 100.0));
        let b = shapes::rectangle(Rect::new(50.0, 50.0, 150.0, 150.0));
        let near = |x: f64, y: f64| (x - y).abs() < 1.0;
        let add = combine(Op::Add, &[a.clone(), b.clone()]).unwrap();
        assert!(near(area(&add), 17500.0), "add {}", area(&add));
        let sub = combine(Op::Subtract, &[a.clone(), b.clone()]).unwrap();
        assert!(near(area(&sub), 7500.0), "subtract {}", area(&sub));
        let int = combine(Op::Intersect, &[a.clone(), b.clone()]).unwrap();
        assert!(near(area(&int), 2500.0), "intersect {}", area(&int));
        assert_eq!(int.bounds().unwrap(), Rect::new(50.0, 50.0, 100.0, 100.0));
        let mb = combine(Op::MinusBack, &[a.clone(), b.clone()]).unwrap();
        assert!(near(area(&mb), 7500.0));
        assert!(mb.bounds().unwrap().x1 > 149.0, "the front square remains");
        let ex = combine(Op::Exclude, &[a.clone(), b]).unwrap();
        assert_eq!(ex.subpaths.len(), 2);
        let far = shapes::rectangle(Rect::new(500.0, 500.0, 510.0, 510.0));
        assert!(combine(Op::Intersect, &[a, far]).is_none());
    }

    #[test]
    fn degenerate_base_shape_leaves_nothing() {
        let dot = PathData::new(vec![SubPath::new(vec![Anchor::corner(Point::new(5.0, 5.0))], false)]);
        let rect = shapes::rectangle(Rect::new(0.0, 0.0, 100.0, 100.0));
        assert!(combine(Op::Subtract, &[dot.clone(), rect.clone()]).is_none());
        assert!(combine(Op::Intersect, &[rect.clone(), dot.clone()]).is_none());
        assert!(combine(Op::MinusBack, &[rect, dot]).is_none());
    }

    #[test]
    fn curves_stay_curves() {
        let a = shapes::ellipse(Rect::new(0.0, 0.0, 100.0, 100.0));
        let b = shapes::ellipse(Rect::new(60.0, 0.0, 160.0, 100.0));
        let add = combine(Op::Add, &[a, b]).unwrap();
        assert_eq!(add.subpaths.len(), 1);
        assert!(add.anchors().any(|(_, _, a)| a.has_in() || a.has_out()), "curved anchors");
        let bb = add.bounds().unwrap();
        assert!((bb.x0 - 0.0).abs() < 0.5 && (bb.x1 - 160.0).abs() < 0.5, "{bb:?}");
    }
}
