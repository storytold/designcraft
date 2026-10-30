//! Object › Fitting › Content-Aware Fit: the graphic fills its frame proportionally, and the part
//! that shows is the one that holds most of what stands out in the image, rather than its middle.
//!
//! What stands out is estimated on a coarse grid of the image: the luminance gradient (edges and
//! texture) plus how far each cell's colour is from the image's average colour. Along the axis the
//! graphic overflows, the frame's window slides to where it holds the most of that.

use designcraft_doc::{Fitting, Graphic};
use designcraft_geom::{Affine, Rect};

/// Images larger than this many pixels aren't analysed (they are filled and aligned as usual).
const MAX_PIXELS: u64 = 40_000_000;
/// The finest grid the image is analysed on.
const GRID: u32 = 96;
/// Pixels sampled at most to build the grid.
const MAX_SAMPLES: u64 = 400_000;

/// A saliency map of raster `data` on a grid of `cols` × `rows` cells (row-major), or None for
/// data that can't be decoded or is too large.
pub(crate) fn saliency(data: &[u8]) -> Option<(usize, usize, Vec<f32>)> {
    let (w, h) = designcraft_images::pixel_size(data)?;
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return None;
    }
    let img = designcraft_images::decode_rgba(data)?;
    if img.width() != w || img.height() != h {
        return None;
    }
    let (cols, rows) = (w.min(GRID) as usize, h.min(GRID) as usize);
    let step = (((u64::from(w) * u64::from(h)) as f64 / MAX_SAMPLES as f64).sqrt().ceil() as u32).max(1);
    let raw = img.as_raw();
    // Each cell's average colour, transparent pixels over white.
    let mut acc = vec![[0.0f64; 4]; cols * rows];
    for y in (0..h).step_by(step as usize) {
        let gy = (y as usize * rows) / h as usize;
        for x in (0..w).step_by(step as usize) {
            let gx = (x as usize * cols) / w as usize;
            let i = (y as usize * w as usize + x as usize) * 4;
            let (Some(p), Some(a)) = (raw.get(i..i + 4), acc.get_mut(gy * cols + gx)) else { continue };
            let alpha = f64::from(p[3]) / 255.0;
            for c in 0..3 {
                a[c] += f64::from(p[c]) / 255.0 * alpha + (1.0 - alpha);
            }
            a[3] += 1.0;
        }
    }
    let cells: Vec<[f64; 3]> = acc.iter().map(|a| if a[3] > 0.0 { [a[0] / a[3], a[1] / a[3], a[2] / a[3]] } else { [1.0; 3] }).collect();
    let n = cells.len().max(1) as f64;
    let mean = cells.iter().fold([0.0; 3], |m, c| [m[0] + c[0] / n, m[1] + c[1] / n, m[2] + c[2] / n]);
    let lum = |c: &[f64; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let at = |x: usize, y: usize| cells.get(y.min(rows - 1) * cols + x.min(cols - 1)).map_or(0.0, lum);
    let mut out = vec![0.0f32; cols * rows];
    for y in 0..rows {
        for x in 0..cols {
            let gx = at(x + 1, y) - at(x.saturating_sub(1), y);
            let gy = at(x, y + 1) - at(x, y.saturating_sub(1));
            let c = cells.get(y * cols + x).copied().unwrap_or([1.0; 3]);
            let dist = ((c[0] - mean[0]).powi(2) + (c[1] - mean[1]).powi(2) + (c[2] - mean[2]).powi(2)).sqrt();
            if let Some(o) = out.get_mut(y * cols + x) {
                *o = ((gx * gx + gy * gy).sqrt() + dist) as f32;
            }
        }
    }
    Some((cols, rows, out))
}

/// Where along a profile what stands out is centred, as a fraction (0..1), or None when nothing
/// stands out from the rest (a plain image). The background level, the profile's median, is taken
/// off first, so a small subject isn't outweighed by a large plain area.
fn centre_of(profile: &[f64]) -> Option<f64> {
    let mut sorted: Vec<f64> = profile.iter().copied().filter(|v| v.is_finite()).collect();
    if sorted.is_empty() {
        return None;
    }
    sorted.sort_by(f64::total_cmp);
    let median = sorted[sorted.len() / 2];
    let (mut sum, mut moment) = (0.0, 0.0);
    for (i, v) in profile.iter().enumerate() {
        let w = (v - median).max(0.0);
        sum += w;
        moment += w * (i as f64 + 0.5);
    }
    let peak = sorted.last().copied().unwrap_or(0.0).abs().max(1e-12);
    (sum > 1e-6 * peak * profile.len() as f64).then(|| moment / sum / profile.len() as f64)
}

/// The transform that fills `frame` with graphic `g` proportionally, keeping the part of the image
/// `map` (from [`saliency`]) marks as standing out in view. Without a map, Fill Frame
/// Proportionally at the graphic's fitting alignment.
pub(crate) fn content_aware(g: &Graphic, frame: Rect, map: Option<&(usize, usize, Vec<f32>)>) -> Option<Affine> {
    let fill = g.fitted(frame, Fitting::FillProportionally)?;
    let Some((cols, rows, s)) = map else { return Some(fill) };
    let (cols, rows) = (*cols, *rows);
    let (nw, nh) = g.size;
    let k = fill.as_coeffs()[0];
    if !(k.is_finite() && k > 0.0) || cols == 0 || rows == 0 || s.len() != cols * rows {
        return Some(fill);
    }
    // The area the graphic fills: the frame grown by the fitting crop, as `Graphic::fitted` uses.
    let [t, l, b, r] = g.crop;
    let area = Rect::new(frame.x0 - l, frame.y0 - t, frame.x1 + r, frame.y1 + b);
    let [_, _, _, _, mut tx, mut ty] = fill.as_coeffs();
    // Across: the columns' totals; down: the rows'.
    if nw * k > area.width() + 1e-6 {
        let profile: Vec<f64> = (0..cols).map(|x| (0..rows).map(|y| f64::from(s[y * cols + x])).sum()).collect();
        if let Some(c) = centre_of(&profile) {
            let start = c * nw - area.width() / k / 2.0;
            tx = area.x0 - start.clamp(0.0, nw - area.width() / k) * k;
        }
    }
    if nh * k > area.height() + 1e-6 {
        let profile: Vec<f64> = (0..rows).map(|y| (0..cols).map(|x| f64::from(s[y * cols + x])).sum()).collect();
        if let Some(c) = centre_of(&profile) {
            let start = c * nh - area.height() / k / 2.0;
            ty = area.y0 - start.clamp(0.0, nh - area.height() / k) * k;
        }
    }
    Some(Affine::translate((tx, ty)) * Affine::scale(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_centre_is_where_the_profile_stands_out_and_none_on_a_flat_one() {
        assert_eq!(centre_of(&[1.0, 1.0, 1.0, 5.0, 5.0, 1.0, 1.0, 1.0]), Some(0.5));
        let c = centre_of(&[1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 9.0, 1.0]).unwrap();
        assert!((c - 6.5 / 8.0).abs() < 1e-9, "{c}");
        assert_eq!(centre_of(&[2.0; 10]), None);
        assert_eq!(centre_of(&[]), None);
    }
}

#[cfg(test)]
mod command_tests {
    use serde_json::json;

    /// A 200 × 100 px image, grey with a red patch where `patch(x)` holds.
    fn png(patch: impl Fn(usize) -> bool) -> Vec<u8> {
        let mut pixels = vec![0u8; 200 * 100 * 4];
        for y in 0..100 {
            for x in 0..200 {
                let i = (y * 200 + x) * 4;
                let red = patch(x) && (30..70).contains(&y);
                pixels[i..i + 4].copy_from_slice(&if red { [220, 30, 30, 255] } else { [128, 128, 128, 255] });
            }
        }
        designcraft_render::Rendered { width: 200, height: 100, pixels }.to_png()
    }

    /// The left edge of the graphic, relative to its 100 pt square frame, after a fit.
    fn left_after(s: &mut crate::Session, data: &[u8], mode: &str) -> f64 {
        let f = s.execute("frame.create", &json!({"rect": [100, 100, 200, 200]})).unwrap()["id"].as_u64().unwrap();
        s.execute("file.place", &json!({"base64": crate::cmd::file::base64_encode(data), "name": "p.png", "frame": f})).unwrap();
        s.execute("object.fit", &json!({"mode": mode, "ids": [f]})).unwrap();
        let it = s.doc().unwrap().doc.item(designcraft_doc::ItemId(f)).unwrap().clone();
        let designcraft_doc::Content::Graphic(g) = &it.content else { panic!("a graphic") };
        let r = g.xf.transform_rect_bbox(designcraft_geom::Rect::new(0.0, 0.0, g.size.0, g.size.1));
        assert!((r.height() - 100.0).abs() < 1e-6, "filled: {r:?}");
        r.x0 - it.inner_bounds().x0
    }

    #[test]
    fn content_aware_fit_shows_what_stands_out_and_centres_a_plain_image() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        // 200 × 100 px at 72 ppi into a 100 pt square: filled, 100 pt of it shows.
        let filled = left_after(&mut s, &png(|x| (150..190).contains(&x)), "fillProportionally");
        assert!((filled + 50.0).abs() < 1e-6, "Fill Frame Proportionally centres: {filled}");
        let aware = left_after(&mut s, &png(|x| (150..190).contains(&x)), "contentAwareFit");
        assert!((aware + 100.0).abs() < 2.5, "the right half, with the red patch, shows: {aware}");
        // One undo step brings back the placed graphic's fit.
        let steps = s.doc().unwrap().history.undo.len();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().history.undo.len(), steps - 1);
        let left = left_after(&mut s, &png(|x| (10..50).contains(&x)), "contentAwareFit");
        assert!(left.abs() < 2.5, "the left half shows: {left}");
        let plain = left_after(&mut s, &png(|_| false), "contentAwareFit");
        assert!((plain + 50.0).abs() < 2.5, "nothing stands out: centred, as Fill Frame Proportionally: {plain}");
    }
}
