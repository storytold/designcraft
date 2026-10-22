//! Tool cursors drawn in code: black glyphs with a white halo, so they read on any page colour.
//! The Selection tool's filled arrow, Direct Selection's hollow arrow, the pen nib with a badge for
//! what a click does (add, delete, close, convert, continue), and a curved double arrow for rotating.
//! Cursors without a glyph stay system cursors.
//!
//! Linux and Windows show the glyphs as OS cursors ([`Images`]): the system moves them itself, so
//! they never trail the pointer. macOS sizes a cursor bitmap in points (a Retina one would show
//! twice as big, or blurred), so there the canvas paints the glyph at the pointer. The
//! web build keeps the system cursors.

use egui::epaint::{Mesh, TessellationOptions, Tessellator, Vertex};
use egui::{Color32, CustomCursorImage, Pos2, Shape, Stroke, Vec2, pos2, vec2};

use designcraft_tools::Cursor;

const INK: Color32 = Color32::BLACK;
const HALO: Color32 = Color32::WHITE;

/// How the window shows a cursor glyph.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// As an OS cursor bitmap ([`Images`]).
    Os,
    /// Painted into the window at the pointer, the OS cursor hidden.
    Painted,
    /// Not at all: the system cursor stands in.
    System,
}

pub const MODE: Mode = if cfg!(target_arch = "wasm32") {
    Mode::System
} else if cfg!(target_os = "macos") {
    Mode::Painted
} else {
    Mode::Os
};

/// The shapes of one glyph: every halo goes under all the ink, so one part's halo never cuts
/// through another part's ink.
#[derive(Default)]
struct Ink {
    halo: Vec<Shape>,
    ink: Vec<Shape>,
}

impl Ink {
    fn halo(&mut self, s: Shape) {
        self.halo.push(s);
    }

    fn add(&mut self, s: Shape) {
        self.ink.push(s);
    }

    /// A filled outline with a halo around it.
    fn poly(&mut self, pts: Vec<Pos2>, fill: Color32) {
        self.halo(Shape::closed_line(pts.clone(), Stroke::new(3.0, HALO)));
        self.add(Shape::convex_polygon(pts.clone(), fill, Stroke::NONE));
        self.add(Shape::closed_line(pts, Stroke::new(1.0, INK)));
    }

    /// A haloed stroke.
    fn line(&mut self, a: Pos2, b: Pos2) {
        self.halo(Shape::line_segment([a, b], Stroke::new(3.0, HALO)));
        self.add(Shape::line_segment([a, b], Stroke::new(1.2, INK)));
    }

    fn into_shapes(self) -> Vec<Shape> {
        let mut out = self.halo;
        out.extend(self.ink);
        out
    }
}

fn at(o: Pos2, pts: &[(f32, f32)]) -> Vec<Pos2> {
    pts.iter().map(|&(x, y)| o + vec2(x, y)).collect()
}

/// The pointer arrow, tip at `o`: ink-filled (Selection) or hollow (Direct Selection).
fn arrow(ink: &mut Ink, o: Pos2, hollow: bool) {
    let pts = at(o, &[(0.0, 0.0), (0.0, 15.0), (3.8, 11.4), (6.4, 17.0), (8.6, 16.0), (6.1, 10.6), (11.0, 10.6)]);
    ink.halo(Shape::closed_line(pts.clone(), Stroke::new(3.0, HALO)));
    // The arrow is concave: fill it as two convex parts, then outline it.
    let fill = if hollow { HALO } else { INK };
    let part = |idx: &[usize]| idx.iter().filter_map(|&i| pts.get(i).copied()).collect::<Vec<_>>();
    ink.add(Shape::convex_polygon(part(&[0, 1, 2, 5, 6]), fill, Stroke::NONE));
    ink.add(Shape::convex_polygon(part(&[2, 3, 4, 5]), fill, Stroke::NONE));
    ink.add(Shape::closed_line(pts, Stroke::new(1.0, INK)));
}

/// What the pen badge says a click does.
#[derive(Clone, Copy)]
enum Badge {
    None,
    Add,
    Delete,
    Close,
    Convert,
    Continue,
}

/// The pen nib, tip at `o` (top left), with a badge below right.
fn pen(ink: &mut Ink, o: Pos2, badge: Badge) {
    ink.poly(at(o, &[(0.0, 0.0), (4.0, 12.0), (8.0, 16.0), (16.0, 8.0), (12.0, 4.0)]), INK);
    // The nib's slit and breather hole.
    ink.add(Shape::line_segment([o + vec2(1.0, 1.0), o + vec2(5.5, 5.5)], Stroke::new(1.0, HALO)));
    ink.add(Shape::circle_filled(o + vec2(6.5, 6.5), 1.4, HALO));
    ink.line(o + vec2(10.0, 14.0), o + vec2(14.0, 18.0));
    let b = o + vec2(16.0, 14.0);
    match badge {
        Badge::None => {}
        Badge::Add => {
            ink.line(b + vec2(0.0, 3.0), b + vec2(6.0, 3.0));
            ink.line(b + vec2(3.0, 0.0), b + vec2(3.0, 6.0));
        }
        Badge::Delete => ink.line(b + vec2(0.0, 3.0), b + vec2(6.0, 3.0)),
        Badge::Close => {
            ink.halo(Shape::circle_stroke(b + vec2(3.0, 3.0), 3.0, Stroke::new(3.0, HALO)));
            ink.add(Shape::circle_stroke(b + vec2(3.0, 3.0), 3.0, Stroke::new(1.2, INK)));
        }
        Badge::Convert => {
            ink.line(b + vec2(0.0, 6.0), b + vec2(3.0, 0.0));
            ink.line(b + vec2(3.0, 0.0), b + vec2(6.0, 6.0));
        }
        Badge::Continue => ink.line(b + vec2(1.0, 6.0), b + vec2(5.0, 0.0)),
    }
}

/// A curved double arrow over the hotspot: an arc bowing up, an arrowhead at each end pointing on
/// along the arc.
fn rotate(ink: &mut Ink, o: Pos2) {
    const R: f32 = 8.0;
    let (a0, a1) = (std::f32::consts::PI * 0.12, std::f32::consts::PI * 0.88);
    let on = |a: f32| o + vec2(a.cos() * R, -a.sin() * R);
    let pts: Vec<Pos2> = (0..=12).map(|i| on(a0 + (a1 - a0) * i as f32 / 12.0)).collect();
    ink.halo(Shape::line(pts.clone(), Stroke::new(3.2, HALO)));
    ink.add(Shape::line(pts, Stroke::new(1.3, INK)));
    for (a, away) in [(a0, -1.0f32), (a1, 1.0)] {
        let t = vec2(-a.sin(), -a.cos()) * away;
        let n = vec2(-t.y, t.x);
        let (end, tip) = (on(a) - t * 1.5, on(a) + t * 4.5);
        ink.poly(vec![tip, end + n * 1.8, end - n * 1.8], INK);
    }
}

/// The shapes of cursor `c` with its hotspot at `p`, in points; `None` for system cursors.
fn glyph(c: Cursor, p: Pos2) -> Option<Vec<Shape>> {
    let mut ink = Ink::default();
    match c {
        Cursor::Arrow => arrow(&mut ink, p, false),
        Cursor::ArrowHollow => arrow(&mut ink, p, true),
        Cursor::Pen => pen(&mut ink, p, Badge::None),
        Cursor::PenAdd => pen(&mut ink, p, Badge::Add),
        Cursor::PenDelete => pen(&mut ink, p, Badge::Delete),
        Cursor::PenClose => pen(&mut ink, p, Badge::Close),
        Cursor::PenConvert => pen(&mut ink, p, Badge::Convert),
        Cursor::PenContinue => pen(&mut ink, p, Badge::Continue),
        Cursor::Rotate => rotate(&mut ink, p),
        _ => return None,
    }
    Some(ink.into_shapes())
}

/// The cursor bitmaps handed to the OS, by (cursor, pixels per point). Each glyph is rasterized
/// once; handing the same `Arc` every frame lets the integration keep its OS cursor.
#[derive(Default)]
pub struct Images(Vec<(Cursor, f32, CustomCursorImage)>);

impl Images {
    /// Cursor `c` for `ppp` physical pixels per point; `None` for cursors without a glyph.
    pub fn get(&mut self, c: Cursor, ppp: f32) -> Option<CustomCursorImage> {
        if let Some((.., image)) = self.0.iter().find(|(k, s, _)| *k == c && *s == ppp) {
            return Some(image.clone());
        }
        let image = rasterize(c, ppp)?;
        // A few glyphs at the scales of the displays the window visited.
        if self.0.len() >= 128 {
            self.0.clear();
        }
        self.0.push((c, ppp, image.clone()));
        Some(image)
    }
}

/// The largest side of a cursor bitmap, in pixels (the glyphs span under 30 points).
const MAX_SIDE: f32 = 256.0;

/// Cursor `c` as a straight-alpha RGBA bitmap at `ppp` pixels per point, tessellated
/// (anti-aliased) as egui would paint it, the hotspot on a pixel corner.
pub(crate) fn rasterize(c: Cursor, ppp: f32) -> Option<CustomCursorImage> {
    if !(ppp.is_finite() && ppp > 0.0) {
        return None;
    }
    let shapes = glyph(c, Pos2::ZERO)?;
    let mut tessellator = Tessellator::new(ppp, TessellationOptions::default(), [1, 1], vec![]);
    let mut mesh = Mesh::default();
    for shape in shapes {
        tessellator.tessellate_shape(shape, &mut mesh);
    }
    let bounds = mesh.calc_bounds();
    // The hotspot must lie inside the bitmap.
    let min = (bounds.min.to_vec2() * ppp).floor().min(Vec2::ZERO);
    let max = (bounds.max.to_vec2() * ppp).ceil().max(Vec2::splat(1.0));
    let size = max - min;
    if !(size.x <= MAX_SIDE && size.y <= MAX_SIDE) {
        return None;
    }
    let (w, h) = (size.x as usize, size.y as usize);
    let mut px = vec![[0.0; 4]; w * h];
    let corner = |v: &Vertex| (v.pos.to_vec2() * ppp - min, v.color);
    for &[i, j, k] in mesh.indices.as_chunks::<3>().0 {
        if let (Some(a), Some(b), Some(c)) = (mesh.vertices.get(i as usize), mesh.vertices.get(j as usize), mesh.vertices.get(k as usize)) {
            fill_triangle(&mut px, w, [corner(a), corner(b), corner(c)]);
        }
    }
    let rgba: Vec<u8> = px
        .iter()
        .flat_map(|&[r, g, b, a]| {
            let k = if a > 0.0 { 1.0 / a } else { 0.0 };
            [r * k, g * k, b * k, a].map(|x| (x * 255.0).round().clamp(0.0, 255.0) as u8)
        })
        .collect();
    Some(CustomCursorImage { rgba: rgba.into(), size: [w as u16, h as u16], hotspot: [-min.x as u16, -min.y as u16] })
}

/// Blend triangle `t` (pixel positions and premultiplied colours, interpolated across it) over `px`
/// (premultiplied RGBA, `w` pixels a row) at the pixel centres it covers, as the GPU blends egui's
/// meshes. A centre on an edge goes to one of the two triangles sharing it, so seams aren't doubled.
fn fill_triangle(px: &mut [[f32; 4]], w: usize, t: [(Vec2, Color32); 3]) {
    let edge = |a: Vec2, b: Vec2, p: Vec2| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
    let [a, mut b, mut c] = t;
    let mut area = edge(a.0, b.0, c.0);
    if area < 0.0 {
        std::mem::swap(&mut b, &mut c);
        area = -area;
    }
    if area.is_nan() || area <= 1e-6 || w == 0 {
        return;
    }
    // Of the two triangles sharing an edge (walking it in opposite directions), one owns it.
    let owns = |u: Vec2, v: Vec2| v.y > u.y || (v.y == u.y && v.x > u.x);
    let h = px.len() / w;
    let lo = a.0.min(b.0).min(c.0).max(Vec2::ZERO);
    let hi = a.0.max(b.0).max(c.0).min(vec2(w as f32, h as f32));
    for y in lo.y.floor() as usize..hi.y.ceil() as usize {
        for x in lo.x.floor() as usize..hi.x.ceil() as usize {
            let p = pos2(x as f32 + 0.5, y as f32 + 0.5).to_vec2();
            let weights = [(b.0, c.0), (c.0, a.0), (a.0, b.0)].map(|(u, v)| (edge(u, v, p), owns(u, v)));
            if !weights.iter().all(|&(e, own)| e > 0.0 || (e == 0.0 && own)) {
                continue;
            }
            let Some(dst) = px.get_mut(y * w + x) else { continue };
            let [wa, wb, wc] = weights.map(|(e, _)| e / area);
            let src: [f32; 4] = std::array::from_fn(|i| {
                let channel = |col: Color32| f32::from(col.to_array()[i]);
                (wa * channel(a.1) + wb * channel(b.1) + wc * channel(c.1)) / 255.0
            });
            let keep = 1.0 - src[3];
            *dst = std::array::from_fn(|i| src[i] + dst[i] * keep);
        }
    }
}

/// The system cursor for `c`: shown where the window has no glyph for it, or no bitmap cursors.
pub fn system(c: Cursor) -> egui::CursorIcon {
    use egui::CursorIcon as C;
    match c {
        Cursor::Arrow | Cursor::ArrowHollow => C::Default,
        Cursor::Move => C::Move,
        Cursor::Crosshair => C::Crosshair,
        Cursor::ResizeH => C::ResizeHorizontal,
        Cursor::ResizeV => C::ResizeVertical,
        Cursor::ResizeNwSe => C::ResizeNwSe,
        Cursor::ResizeNeSw => C::ResizeNeSw,
        Cursor::Rotate => C::Alias,
        Cursor::Pen | Cursor::PenAdd | Cursor::PenDelete | Cursor::PenClose | Cursor::PenContinue | Cursor::PenConvert => C::Crosshair,
        Cursor::PathBracket => C::ResizeColumn,
        Cursor::Text => C::Text,
        Cursor::Hand => C::Grab,
        Cursor::HandGrab => C::Grabbing,
        Cursor::ZoomIn => C::ZoomIn,
        Cursor::ZoomOut => C::ZoomOut,
        Cursor::Eyedropper => C::Crosshair,
        Cursor::LoadedText | Cursor::LoadedGraphic => C::Copy,
        Cursor::ThreadLink => C::Alias,
        Cursor::Unthread => C::NoDrop,
        Cursor::NotAllowed => C::NotAllowed,
    }
}

/// Show cursor `c` with the pointer at `hover` (screen points): set the OS cursor bitmap or paint
/// the glyph per [`MODE`], and return the cursor icon to set.
pub fn show(ctx: &egui::Context, images: &mut Images, c: Cursor, hover: Pos2) -> egui::CursorIcon {
    match MODE {
        Mode::Os => {
            // The glyphs keep the system cursors' size whatever the interface scale.
            let ppp = ctx.pixels_per_point() / ctx.zoom_factor().max(0.01);
            ctx.set_cursor_image(images.get(c, ppp));
            system(c)
        }
        Mode::Painted => {
            let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("tool-cursor"));
            // Divide by the zoom factor so the glyph keeps its size in physical pixels.
            let scale = 1.0 / ctx.zoom_factor().max(0.01);
            let painter = ctx.layer_painter(layer);
            match glyph(c, Pos2::ZERO) {
                Some(shapes) => {
                    let place = egui::emath::TSTransform::new(hover.to_vec2(), scale);
                    painter.extend(shapes.into_iter().map(|mut s| {
                        s.transform(place);
                        s
                    }));
                    egui::CursorIcon::None
                }
                None => system(c),
            }
        }
        Mode::System => system(c),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every cursor with a glyph.
    const GLYPHS: [Cursor; 9] = [
        Cursor::Arrow,
        Cursor::ArrowHollow,
        Cursor::Pen,
        Cursor::PenAdd,
        Cursor::PenDelete,
        Cursor::PenClose,
        Cursor::PenConvert,
        Cursor::PenContinue,
        Cursor::Rotate,
    ];

    fn alpha(img: &CustomCursorImage, x: usize, y: usize) -> u8 {
        img.rgba.get((y * img.size[0] as usize + x) * 4 + 3).copied().unwrap_or(0)
    }

    #[test]
    fn every_glyph_rasterizes_with_its_hotspot_inside() {
        for c in GLYPHS {
            for ppp in [1.0, 2.0] {
                let img = rasterize(c, ppp).unwrap_or_else(|| panic!("{c:?} has no bitmap"));
                let [w, h] = img.size.map(usize::from);
                assert_eq!(img.rgba.len(), w * h * 4, "{c:?}");
                let [hx, hy] = img.hotspot.map(usize::from);
                assert!(hx < w && hy < h, "{c:?} hotspot {:?} outside {:?}", img.hotspot, img.size);
                let inked = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).filter(|&(x, y)| alpha(&img, x, y) > 128).count();
                assert!(inked > 20, "{c:?} at {ppp}× is nearly empty ({inked} pixels)");
            }
        }
        for c in [Cursor::Text, Cursor::Hand, Cursor::ZoomIn, Cursor::Crosshair] {
            assert!(rasterize(c, 1.0).is_none(), "{c:?} stays a system cursor");
        }
        assert!(rasterize(Cursor::Pen, f32::NAN).is_none() && rasterize(Cursor::Pen, 0.0).is_none());
    }

    #[test]
    fn the_selection_arrows_differ_in_fill() {
        // A pixel inside the arrow's body, below the tip: ink for Selection, halo for Direct.
        let centre = |c| {
            let img = rasterize(c, 2.0).unwrap_or_else(|| panic!("{c:?}"));
            let [hx, hy] = img.hotspot.map(usize::from);
            let i = ((hy + 16) * img.size[0] as usize + hx + 4) * 4;
            img.rgba.get(i..i + 4).map(|p| p.to_vec()).unwrap_or_default()
        };
        assert_eq!(centre(Cursor::Arrow), vec![0, 0, 0, 255], "Selection is filled");
        assert_eq!(centre(Cursor::ArrowHollow), vec![255, 255, 255, 255], "Direct Selection is hollow");
    }

    #[test]
    fn images_are_cached_by_cursor_and_scale() {
        let mut images = Images::default();
        let a = images.get(Cursor::Pen, 2.0).unwrap_or_else(|| panic!("pen"));
        let b = images.get(Cursor::Pen, 2.0).unwrap_or_else(|| panic!("pen"));
        assert!(std::sync::Arc::ptr_eq(&a.rgba, &b.rgba), "the same bitmap every frame");
        assert_ne!(images.get(Cursor::Pen, 1.0).map(|i| i.size), Some(a.size));
        assert!(images.get(Cursor::Text, 2.0).is_none());
    }
}
