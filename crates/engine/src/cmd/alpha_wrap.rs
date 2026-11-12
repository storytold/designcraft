//! Text Wrap › Contour › Alpha Channel: the outline of a placed raster's visible pixels.
//!
//! The outline is traced once per image and threshold, in the image's pixel space, and kept on
//! the document ([`designcraft_doc::AlphaOutline`]). Composition maps it through the graphic's
//! current transform, so it follows the image when it is moved, scaled or refitted.
use std::sync::Arc;

use designcraft_doc::{AlphaOutline, AssetId, ContourType, Document, WrapMode};
use designcraft_geom::{BezPath, PathData, Point};

/// Images larger than this many pixels aren't traced (their contour follows the frame).
const MAX_PIXELS: u64 = 40_000_000;
/// The finest grid an outline is traced on; coarser grids when the outline would be too long.
const MAX_GRID: u32 = 256;
/// Boundary edges an outline may have.
const MAX_EDGES: usize = 1 << 16;

/// Trace the outlines new Alpha Channel contours need and drop the ones nothing uses. Cheap when
/// nothing changed: tracing happens only for an image and threshold not seen before.
pub(crate) fn sync(doc: &mut Document) {
    let mut wanted: Vec<(AssetId, u8)> = Vec::new();
    for sp in doc.spreads.iter().chain(doc.parents.iter()) {
        // Every frame, in groups and Paste Into containers too, with the wrap set on the frame or
        // on its placed graphic (Direct Selection sets the graphic's).
        for top in &sp.items {
            top.walk(&mut |it| {
                let Some(g) = it.graphic() else { return };
                for w in [&it.wrap, &g.wrap] {
                    if w.mode == WrapMode::Contour && w.contour == ContourType::AlphaChannel && !wanted.contains(&(g.asset, w.alpha_threshold)) {
                        wanted.push((g.asset, w.alpha_threshold));
                    }
                }
            });
        }
    }
    if wanted.is_empty() && doc.alpha_outlines.is_empty() {
        return;
    }
    // An outline traced from data the asset no longer has (relinked or updated) is traced again.
    let assets = &doc.assets;
    doc.alpha_outlines.retain(|o| wanted.contains(&(o.asset, o.threshold)) && assets.get(&o.asset).is_some_and(|a| o.traced_from(&a.data)));
    for (asset, threshold) in wanted {
        if doc.alpha_outline(asset, threshold).is_some() {
            continue;
        }
        let Some(a) = doc.assets.get(&asset).filter(|a| a.is_raster()) else { continue };
        // An image with no outline (opaque, unreadable, too large) is remembered as an empty
        // one, so later edits don't decode it again; the contour is then the frame's.
        let o = trace(&a.data, asset, threshold).unwrap_or_else(|| no_outline(&a.data, asset, threshold));
        doc.alpha_outlines.push(Arc::new(o));
    }
}

/// The empty outline standing for an image `trace` found none in: zero-size, which composition
/// takes as no outline, and dropped like a traced one when the asset's data changes.
fn no_outline(data: &Arc<Vec<u8>>, asset: AssetId, threshold: u8) -> AlphaOutline {
    AlphaOutline { asset, threshold, width: 0, height: 0, outer: Default::default(), holes: Default::default(), source: Arc::downgrade(data) }
}

/// The outline of the pixels of `data` more opaque than `threshold`. None for an image that
/// can't be read, is too large, or has no transparency (its contour is its rectangle).
pub(crate) fn trace(data: &Arc<Vec<u8>>, asset: AssetId, threshold: u8) -> Option<AlphaOutline> {
    let (w, h) = designcraft_images::pixel_size(data)?;
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return None;
    }
    let img = designcraft_images::decode_rgba(data)?;
    if img.width() != w || img.height() != h {
        return None;
    }
    let mut grid = MAX_GRID;
    while grid >= 16 {
        let (cols, rows) = (w.min(grid), h.min(grid));
        let (mask, transparent) = occupancy(img.as_raw(), w, h, cols, rows, threshold);
        if !transparent {
            return None;
        }
        if let Some((outer, holes)) = rings(&mask, cols as usize, rows as usize) {
            let (sx, sy) = (f64::from(w) / f64::from(cols), f64::from(h) / f64::from(rows));
            let path = |rings: &[Vec<(i32, i32)>]| {
                let mut p = BezPath::new();
                for ring in rings {
                    let mut pts = ring.iter().map(|&(x, y)| Point::new(f64::from(x) * sx, f64::from(y) * sy));
                    let Some(first) = pts.next() else { continue };
                    p.move_to(first);
                    for q in pts {
                        p.line_to(q);
                    }
                    p.close_path();
                }
                PathData::from_bezpath(&p)
            };
            let source = Arc::downgrade(data);
            return Some(AlphaOutline { asset, threshold, width: w, height: h, outer: path(&outer), holes: path(&holes), source });
        }
        grid /= 2;
    }
    None
}

/// Which cells of a `cols` × `rows` grid hold a pixel of the `w` × `h` RGBA image `rgba` more
/// opaque than `threshold` (a cell is visible when any of its pixels is), and whether the image
/// has any transparency.
fn occupancy(rgba: &[u8], w: u32, h: u32, cols: u32, rows: u32, threshold: u8) -> (Vec<bool>, bool) {
    let mut mask = vec![false; (cols as usize) * (rows as usize)];
    let mut transparent = false;
    for (i, px) in rgba.as_chunks::<4>().0.iter().enumerate() {
        let alpha = px[3];
        transparent |= alpha < 255;
        if alpha > threshold {
            let (x, y) = ((i as u64) % u64::from(w), (i as u64) / u64::from(w));
            let (gx, gy) = (x * u64::from(cols) / u64::from(w), y * u64::from(rows) / u64::from(h));
            if let Some(c) = mask.get_mut((gy * u64::from(cols) + gx) as usize) {
                *c = true;
            }
        }
    }
    (mask, transparent)
}

/// The closed boundaries of the visible cells, split into outer boundaries and the boundaries of
/// holes (by winding: each boundary keeps the visible cells on the same side). None when the
/// outline would have more than [`MAX_EDGES`] edges.
#[allow(clippy::type_complexity)]
fn rings(mask: &[bool], cols: usize, rows: usize) -> Option<(Vec<Vec<(i32, i32)>>, Vec<Vec<(i32, i32)>>)> {
    use std::collections::BTreeMap;
    let visible = |x: i32, y: i32| {
        x >= 0 && y >= 0 && (x as usize) < cols && (y as usize) < rows && mask.get(y as usize * cols + x as usize).copied().unwrap_or(false)
    };
    let mut edges: BTreeMap<(i32, i32), Vec<(i32, i32)>> = BTreeMap::new();
    let mut count = 0usize;
    for y in 0..rows as i32 {
        for x in 0..cols as i32 {
            if !visible(x, y) {
                continue;
            }
            for (open, a, b) in [
                (!visible(x, y - 1), (x, y), (x + 1, y)),
                (!visible(x + 1, y), (x + 1, y), (x + 1, y + 1)),
                (!visible(x, y + 1), (x + 1, y + 1), (x, y + 1)),
                (!visible(x - 1, y), (x, y + 1), (x, y)),
            ] {
                if open {
                    count += 1;
                    if count > MAX_EDGES {
                        return None;
                    }
                    edges.entry(a).or_default().push(b);
                }
            }
        }
    }
    let (mut outer, mut holes) = (Vec::new(), Vec::new());
    while let Some((&start, _)) = edges.first_key_value() {
        let mut ring = vec![start];
        let mut at = start;
        for _ in 0..=MAX_EDGES {
            let next = edges.get_mut(&at).and_then(Vec::pop)?;
            if edges.get(&at).is_some_and(Vec::is_empty) {
                edges.remove(&at);
            }
            // Drop the middle of straight runs.
            if let [.., a, b] = ring[..]
                && (b.0 - a.0) * (next.1 - b.1) == (b.1 - a.1) * (next.0 - b.0)
            {
                ring.pop();
            }
            if next == start {
                break;
            }
            ring.push(next);
            at = next;
        }
        // Shoelace in y-down space: positive for outer boundaries, negative for holes.
        let area: i64 = ring
            .iter()
            .zip(ring.iter().cycle().skip(1))
            .map(|(&(x0, y0), &(x1, y1))| i64::from(x0) * i64::from(y1) - i64::from(x1) * i64::from(y0))
            .sum();
        if ring.len() >= 3 {
            if area > 0 { outer.push(ring) } else { holes.push(ring) }
        }
    }
    Some((outer, holes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::Session;

    /// A 64 × 64 PNG: visible where `on(x, y)`.
    fn png(on: impl Fn(usize, usize) -> bool) -> Vec<u8> {
        let mut pixels = vec![0; 64 * 64 * 4];
        for y in 0..64 {
            for x in 0..64 {
                let i = (y * 64 + x) * 4;
                pixels[i] = 120;
                pixels[i + 3] = if on(x, y) { 255 } else { 0 };
            }
        }
        designcraft_render::Rendered { width: 64, height: 64, pixels }.to_png()
    }

    #[test]
    fn traces_outer_boundaries_and_holes_apart() {
        // A ring: a square with a square hole.
        let data = png(|x, y| (8..56).contains(&x) && (8..56).contains(&y) && !((24..40).contains(&x) && (24..40).contains(&y)));
        let o = trace(&Arc::new(data), AssetId(1), 25).unwrap();
        assert_eq!((o.width, o.height), (64, 64));
        assert_eq!((o.outer.subpaths.len(), o.holes.subpaths.len()), (1, 1));
        let (a, b) = (o.outer.bounds().unwrap(), o.holes.bounds().unwrap());
        assert_eq!((a.x0, a.y0, a.x1, a.y1), (8.0, 8.0, 56.0, 56.0));
        assert_eq!((b.x0, b.y0, b.x1, b.y1), (24.0, 24.0, 40.0, 40.0));
        // An opaque image has no alpha outline.
        assert!(trace(&Arc::new(png(|_, _| true)), AssetId(1), 25).is_none());
    }

    #[test]
    fn an_alpha_channel_contour_is_traced_when_set_and_kept_out_of_the_saved_file() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let data = png(|x, _| (16..48).contains(&x));
        let id = s
            .execute("file.place", &json!({"base64": crate::cmd::file::base64_encode(&data), "name": "cut.png", "x": 100, "y": 100, "width": 128}))
            .unwrap()["id"]
            .clone();
        assert!(s.doc().unwrap().doc.alpha_outlines.is_empty());
        s.execute("object.textWrap", &json!({"ids": [id], "mode": "contour", "contour": "alphaChannel"})).unwrap();
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.alpha_outlines.len(), 1);
        // Saved and opened again: traced again on open.
        let bytes = crate::cmd::file::to_bytes(d).unwrap();
        let back = crate::cmd::file::from_bytes(&bytes).unwrap();
        assert!(back.alpha_outlines.is_empty(), "not saved");
        let st = crate::DocState::new(back, None);
        assert_eq!(st.doc.alpha_outlines.len(), 1, "traced on open");
        // Wrap off: the outline goes.
        s.execute("object.textWrap", &json!({"ids": [id], "mode": "none"})).unwrap();
        assert!(s.doc().unwrap().doc.alpha_outlines.is_empty());
    }

    /// An image with no outline (here opaque) is remembered as an empty one: later edits don't
    /// trace it again, and the contour is the frame's.
    #[test]
    fn an_image_with_no_outline_is_not_traced_on_every_edit() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let data = png(|_, _| true);
        let id = s
            .execute("file.place", &json!({"base64": crate::cmd::file::base64_encode(&data), "name": "solid.png", "x": 100, "y": 100, "width": 128}))
            .unwrap()["id"]
            .clone();
        s.execute("object.textWrap", &json!({"ids": [id], "mode": "contour", "contour": "alphaChannel"})).unwrap();
        let first = s.doc().unwrap().doc.alpha_outlines.clone();
        assert_eq!(first.len(), 1, "remembered");
        assert_eq!((first[0].width, first[0].height), (0, 0), "as no outline");
        s.execute("frame.create", &json!({"rect": [300, 300, 350, 350]})).unwrap();
        let after = &s.doc().unwrap().doc.alpha_outlines;
        assert!(Arc::ptr_eq(&first[0], &after[0]), "kept, not traced again");
    }

    /// Outlines are traced for a wrap set on the placed graphic and for frames inside groups, and
    /// traced again when the image's data changes under the same asset (relink, link update).
    #[test]
    fn graphic_wraps_nested_frames_and_relinked_images_get_outlines() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let place = |s: &mut Session, x: u64| {
            let data = png(|x, _| (16..48).contains(&x));
            s.execute("file.place", &json!({"base64": crate::cmd::file::base64_encode(&data), "name": "cut.png", "x": x, "y": 100, "width": 128}))
                .unwrap()["id"]
                .as_u64()
                .unwrap()
        };
        let image = place(&mut s, 100);
        // The wrap on the graphic (Direct Selection), not the frame.
        s.execute("object.textWrap", &json!({"ids": [image], "content": true, "mode": "contour", "contour": "alphaChannel"})).unwrap();
        let frame_wrap = s.doc().unwrap().doc.item(designcraft_doc::ItemId(image)).unwrap().wrap.mode;
        assert_eq!(frame_wrap, WrapMode::None, "the frame has no wrap of its own");
        assert_eq!(s.doc().unwrap().doc.alpha_outlines.len(), 1, "traced for the graphic's wrap");
        // In a group with another frame: still traced.
        let other = s.execute("frame.create", &json!({"rect": [300, 100, 350, 150]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.group", &json!({"ids": [image, other]})).unwrap();
        let d = &s.doc().unwrap().doc;
        assert!(d.find(designcraft_doc::ItemId(image)).unwrap().path.len() > 1, "the frame is nested");
        assert_eq!(d.alpha_outlines.len(), 1, "a nested frame keeps its outline");
        // A frame placed and grouped before its wrap is set: traced too.
        let second = place(&mut s, 400);
        let other2 = s.execute("frame.create", &json!({"rect": [600, 100, 650, 150]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.group", &json!({"ids": [second, other2]})).unwrap();
        s.execute("object.textWrap", &json!({"ids": [second], "mode": "contour", "contour": "alphaChannel", "threshold": 200})).unwrap();
        assert_eq!(s.doc().unwrap().doc.alpha_outlines.len(), 2);
        // The image data replaced under the same asset, as a relink does: traced again.
        let Some(designcraft_doc::Content::Graphic(g)) = s.doc().unwrap().doc.item(designcraft_doc::ItemId(image)).map(|it| it.content.clone())
        else {
            panic!("a graphic")
        };
        let narrower = Arc::new(png(|x, _| (24..40).contains(&x)));
        s.edit(|d, _| {
            let a = d.assets.get(&g.asset).unwrap();
            let mut a = (**a).clone();
            a.data = narrower.clone();
            d.assets.insert(g.asset, Arc::new(a));
            Ok(())
        })
        .unwrap();
        let d = &s.doc().unwrap().doc;
        let o = d.alpha_outline(g.asset, designcraft_doc::DEFAULT_ALPHA_THRESHOLD).unwrap();
        assert!(o.traced_from(&narrower));
        let b = o.outer.bounds().unwrap();
        assert_eq!((b.x0, b.x1), (24.0, 40.0), "the new image's outline");
    }

    /// An image placed 128 pt wide at (100, 100) (2 pt per pixel), its wrap a contour with no
    /// offset, and a caption frame at `caption` with 8 pt "Hi".
    fn page(data: &[u8], caption: [f64; 4]) -> (Session, Value, u64) {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let image = s
            .execute("file.place", &json!({"base64": crate::cmd::file::base64_encode(data), "name": "cut.png", "x": 100, "y": 100, "width": 128}))
            .unwrap()["id"]
            .clone();
        let c = s.execute("frame.create", &json!({"rect": caption, "content": "text", "text": "Hi"})).unwrap();
        s.execute("selection.set", &json!({"ids": [c["id"]]})).unwrap();
        s.execute("type.char", &json!({"size": 8})).unwrap();
        s.execute("object.textWrap", &json!({"ids": [image], "mode": "contour", "offset": 0})).unwrap();
        (s, image, c["story"].as_u64().unwrap())
    }

    fn overset(s: &Session, story: u64) -> bool {
        s.cache.get(&s.doc().unwrap().doc, designcraft_doc::StoryId(story), None).is_overset()
    }

    use serde_json::Value;

    /// A caption fits between two legs of a cutout, and the contour follows the image when it's
    /// moved within its frame (it isn't a boundary stored where it was traced).
    #[test]
    fn an_alpha_channel_contour_keeps_gaps_open_and_follows_the_image() {
        // Legs at pixels 8..20 and 44..56: 116..140 and 188..212 on the page.
        let (mut s, image, story) = page(&png(|x, _| (8..20).contains(&x) || (44..56).contains(&x)), [150.0, 120.0, 178.0, 170.0]);
        assert!(overset(&s, story), "the frame's contour covers the gap");
        s.execute("object.textWrap", &json!({"ids": [image], "contour": "alphaChannel"})).unwrap();
        assert!(!overset(&s, story), "the gap between the legs stays open");
        // The graphic moved 30 pt right within its frame: its left leg now covers the caption.
        s.execute("selection.set", &json!({"ids": [image], "content": true})).unwrap();
        s.execute("transform.move", &json!({"dx": 30, "dy": 0})).unwrap();
        assert!(overset(&s, story), "the contour moved with the image");
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(!overset(&s, story));
    }

    /// A hole enclosed by the image stays blocked unless Include Inside Edges is on.
    #[test]
    fn holes_are_blocked_unless_inside_edges_are_included() {
        // A ring: outer 116..212, hole 148..180 on the page.
        let ring = png(|x, y| (8..56).contains(&x) && (8..56).contains(&y) && !((24..40).contains(&x) && (24..40).contains(&y)));
        let (mut s, image, story) = page(&ring, [152.0, 152.0, 176.0, 176.0]);
        s.execute("object.textWrap", &json!({"ids": [image], "contour": "alphaChannel"})).unwrap();
        assert!(overset(&s, story), "the hole is blocked with the ring around it");
        s.execute("object.textWrap", &json!({"ids": [image], "insideEdges": true})).unwrap();
        assert!(!overset(&s, story), "Include Inside Edges lets text into the hole");
        for bad in [json!({"ids": [image], "threshold": 255}), json!({"ids": [image], "insideEdges": 1})] {
            assert!(s.execute("object.textWrap", &bad).is_err());
        }
    }
}
