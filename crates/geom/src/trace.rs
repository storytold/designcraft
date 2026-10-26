//! Outline tracing for clipping paths: the boundary of the "inside" pixels of a mask, as closed
//! polygons in pixel coordinates, simplified.

use std::collections::HashMap;

use crate::Point;

/// Pixel-border edges of the mask with the inside on the right (clockwise, y down), by start point.
fn border_edges(mask: &[bool], w: usize, h: usize) -> HashMap<(isize, isize), Vec<(isize, isize)>> {
    let inside = |x: isize, y: isize| x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && mask[y as usize * w + x as usize];
    let mut next: HashMap<(isize, isize), Vec<(isize, isize)>> = HashMap::new();
    let mut add = |a: (isize, isize), b: (isize, isize)| next.entry(a).or_default().push(b);
    for y in 0..h as isize {
        for x in 0..w as isize {
            if !inside(x, y) {
                continue;
            }
            if !inside(x, y - 1) {
                add((x, y), (x + 1, y));
            }
            if !inside(x + 1, y) {
                add((x + 1, y), (x + 1, y + 1));
            }
            if !inside(x, y + 1) {
                add((x + 1, y + 1), (x, y + 1));
            }
            if !inside(x - 1, y) {
                add((x, y + 1), (x, y));
            }
        }
    }
    next
}

/// Closed outlines around the `true` cells of a `w`×`h` mask (row-major), simplified to within
/// `tol` pixels; outlines enclosing less than `min_area` pixels are dropped. Holes come out
/// with the opposite winding.
pub fn trace(mask: &[bool], w: usize, h: usize, tol: f64, min_area: f64) -> Vec<Vec<Point>> {
    let mut next = border_edges(mask, w, h);
    let mut out = Vec::new();
    let mut starts: Vec<(isize, isize)> = next.keys().copied().collect();
    starts.sort();
    for s in starts {
        while let Some(first) = next.get_mut(&s).and_then(Vec::pop) {
            let mut ring = vec![s];
            let mut cur = first;
            while cur != s {
                ring.push(cur);
                match next.get_mut(&cur).and_then(Vec::pop) {
                    Some(n) => cur = n,
                    None => break,
                }
            }
            let pts: Vec<Point> = ring.into_iter().map(|(x, y)| Point::new(x as f64, y as f64)).collect();
            if area(&pts).abs() >= min_area {
                out.push(simplify_closed(&pts, tol));
            }
        }
    }
    out
}

fn area(p: &[Point]) -> f64 {
    let n = p.len();
    (0..n).map(|i| p[i].x * p[(i + 1) % n].y - p[(i + 1) % n].x * p[i].y).sum::<f64>() / 2.0
}

/// Ramer–Douglas–Peucker on a closed ring (split at its two farthest-apart points).
fn simplify_closed(p: &[Point], tol: f64) -> Vec<Point> {
    if p.len() < 4 {
        return p.to_vec();
    }
    let far = (1..p.len()).max_by(|a, b| (p[*a] - p[0]).hypot().total_cmp(&(p[*b] - p[0]).hypot())).unwrap_or(p.len() / 2);
    let mut a = rdp(&p[..=far], tol);
    let mut back: Vec<Point> = p[far..].to_vec();
    back.push(p[0]);
    let b = rdp(&back, tol);
    a.pop();
    a.extend(b);
    a.pop();
    a
}

fn rdp(p: &[Point], tol: f64) -> Vec<Point> {
    if p.len() < 3 {
        return p.to_vec();
    }
    let (a, b) = (p[0], p[p.len() - 1]);
    let d = b - a;
    let len = d.hypot();
    let (mut best, mut idx) = (0.0, 0);
    for (i, q) in p.iter().enumerate().take(p.len() - 1).skip(1) {
        let dist = if len < 1e-12 { (*q - a).hypot() } else { ((*q - a).cross(d) / len).abs() };
        if dist > best {
            (best, idx) = (dist, i);
        }
    }
    if best <= tol {
        return vec![a, b];
    }
    let mut left = rdp(&p[..=idx], tol);
    left.pop();
    left.extend(rdp(&p[idx..], tol));
    left
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traces_a_disc_and_its_hole() {
        let (w, h) = (40, 40);
        let mask: Vec<bool> = (0..w * h)
            .map(|i| {
                let (x, y) = ((i % w) as f64 + 0.5 - 20.0, (i / w) as f64 + 0.5 - 20.0);
                let r = (x * x + y * y).sqrt();
                r < 15.0 && r > 6.0
            })
            .collect();
        let rings = trace(&mask, w, h, 0.75, 4.0);
        assert_eq!(rings.len(), 2, "outer edge and hole");
        let areas: Vec<f64> = rings.iter().map(|r| area(r)).collect();
        assert!(areas.iter().any(|a| *a > 600.0) && areas.iter().any(|a| *a < -80.0), "{areas:?}");
        assert!(rings.iter().all(|r| r.len() < 60), "simplified");
        assert!(trace(&[false; 16], 4, 4, 1.0, 1.0).is_empty());
    }
}
