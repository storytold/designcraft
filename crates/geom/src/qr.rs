//! QR codes as vector paths (Object › Generate QR Code), with a style: the shape of the modules
//! and of the three finder patterns, the error correction level, and an area left clear in the
//! middle for a logo.

use kurbo::Rect;
use serde::{Deserialize, Serialize};

use crate::Point;
use crate::path::{Anchor, PathData, SubPath};

/// Error correction: how much of the code may be damaged or covered (by a logo) and still read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Ecc {
    /// About 7 %.
    Low,
    /// About 15 %, as InDesign uses for print.
    #[default]
    Medium,
    /// About 25 %.
    Quartile,
    /// About 30 %: for a logo in the middle.
    High,
}

impl Ecc {
    fn level(self) -> qrcodegen::QrCodeEcc {
        match self {
            Ecc::Low => qrcodegen::QrCodeEcc::Low,
            Ecc::Medium => qrcodegen::QrCodeEcc::Medium,
            Ecc::Quartile => qrcodegen::QrCodeEcc::Quartile,
            Ecc::High => qrcodegen::QrCodeEcc::High,
        }
    }
}

/// How the dark modules are drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Modules {
    /// Squares, touching (the classic code).
    #[default]
    Square,
    /// Squares with rounded corners, each on its own.
    Rounded,
    /// Dots.
    Dots,
    /// Rounded where a module has no dark neighbour, so neighbouring modules flow into one shape.
    Connected,
}

/// How the three finder patterns (the large squares in the corners) are drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Finder {
    #[default]
    Square,
    Rounded,
    Circle,
}

/// A QR code's look. The defaults draw the classic code.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct QrStyle {
    pub ecc: Ecc,
    pub modules: Modules,
    /// How round the modules' corners are, 0 (square) to 1 (as round as they get).
    pub roundness: f64,
    /// A module's size in its cell, 0.5 to 1: below 1 the modules stand apart.
    pub scale: f64,
    pub finder: Finder,
    /// The side of the area left clear in the middle, as a fraction of the code's side (0 to
    /// [`MAX_LOGO`]): room for a logo.
    pub logo: f64,
}

impl Default for QrStyle {
    fn default() -> Self {
        QrStyle { ecc: Ecc::Medium, modules: Modules::Square, roundness: 1.0, scale: 1.0, finder: Finder::Square, logo: 0.0 }
    }
}

/// The largest logo area: a third of the side, about a ninth of the code, which High error
/// correction still reads around.
pub const MAX_LOGO: f64 = 0.34;

impl QrStyle {
    /// The style with every number in its range (a hostile file or command can't break the code).
    pub fn sanitized(self) -> Self {
        let clamp = |v: f64, lo: f64, hi: f64, def: f64| if v.is_finite() { v.clamp(lo, hi) } else { def };
        QrStyle {
            roundness: clamp(self.roundness, 0.0, 1.0, 1.0),
            scale: clamp(self.scale, 0.5, 1.0, 1.0),
            logo: clamp(self.logo, 0.0, MAX_LOGO, 0.0),
            ..self
        }
    }

    /// Whether this is the classic code (no stored style needed).
    pub fn is_classic(&self) -> bool {
        *self == QrStyle::default()
    }
}

/// Which part of a QR code an object is, besides the code itself (which carries the style): a
/// code drawn in more than one colour is a group of these, each an ordinary filled object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QrPart {
    /// The frame the code sits in: its fill is the background behind the code and its quiet
    /// zone, its stroke the code's outline. (Earlier: a square of its own behind the code.)
    Background,
    /// The three finder patterns, in a colour of their own.
    Finders,
    /// The frame in the middle holding the logo image.
    Logo,
}

/// A QR code's shapes: the data modules, the three finder patterns, and where a logo goes.
#[derive(Clone, Debug, PartialEq)]
pub struct QrShapes {
    /// The code with its quiet zone: the background's square.
    pub frame: Rect,
    pub modules: PathData,
    pub finders: PathData,
    /// The clear area in the middle, inset by half a module: where a logo fits.
    pub logo: Option<Rect>,
    /// Modules per side.
    pub size: i32,
}

/// Quiet zone, modules each side.
const QUIET: i32 = 2;

/// The QR code for `text` in `r` (square, centred, a quiet zone of two modules each side) drawn
/// in `style`. `None` if the text is too long for any QR version at its error correction.
pub fn qr_shapes(text: &str, r: Rect, style: &QrStyle) -> Option<QrShapes> {
    let style = style.sanitized();
    // A rectangle given corner to corner the other way round is the same square; an empty or
    // non-finite one holds no code.
    let r = r.abs();
    let side = r.width().min(r.height());
    if ![r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite()) || side <= 0.0 {
        return None;
    }
    let qr = qrcodegen::QrCode::encode_text(text, style.ecc.level()).ok()?;
    let n = qr.size();
    let m = side / (n + 2 * QUIET) as f64;
    let origin = Point::new(r.center().x - side / 2.0 + QUIET as f64 * m, r.center().y - side / 2.0 + QUIET as f64 * m);
    let cell =
        |x: i32, y: i32| Rect::new(origin.x + x as f64 * m, origin.y + y as f64 * m, origin.x + (x + 1) as f64 * m, origin.y + (y + 1) as f64 * m);
    // The finder patterns with their separators: drawn on their own.
    let in_finder = |x: i32, y: i32| (x < 8 && y < 8) || (x >= n - 8 && y < 8) || (x < 8 && y >= n - 8);
    // The clear area: an odd number of modules, centred.
    let clear = {
        let k = ((style.logo * n as f64).round() as i32) | 1;
        let lo = (n - k) / 2;
        (style.logo > 0.0).then_some((lo, lo + k))
    };
    let cleared = |x: i32, y: i32| clear.is_some_and(|(lo, hi)| x >= lo && x < hi && y >= lo && y < hi);
    let dark = |x: i32, y: i32| x >= 0 && y >= 0 && x < n && y < n && qr.get_module(x, y) && !in_finder(x, y) && !cleared(x, y);
    let mut modules = Vec::new();
    match style.modules {
        Modules::Square if style.scale >= 1.0 => {
            // One rectangle per horizontal run.
            for y in 0..n {
                let mut x = 0;
                while x < n {
                    if !dark(x, y) {
                        x += 1;
                        continue;
                    }
                    let x0 = x;
                    while x < n && dark(x, y) {
                        x += 1;
                    }
                    modules.push(round_rect(cell(x0, y).union(cell(x - 1, y)), [0.0; 4]));
                }
            }
        }
        shape => {
            for y in 0..n {
                for x in 0..n {
                    if !dark(x, y) {
                        continue;
                    }
                    let c = cell(x, y);
                    let s = style.scale * m;
                    let r = Rect::from_center_size(c.center(), (s, s));
                    let full = s / 2.0;
                    let radii = match shape {
                        Modules::Square => [0.0; 4],
                        Modules::Rounded => [style.roundness * full; 4],
                        Modules::Dots => [full; 4],
                        // A corner rounds only where neither neighbour towards it is dark, so
                        // runs and blocks of modules flow into one shape.
                        Modules::Connected => {
                            let rr = style.roundness * full;
                            let open = |dx: i32, dy: i32| !dark(x + dx, y) && !dark(x, y + dy);
                            [open(-1, -1), open(1, -1), open(1, 1), open(-1, 1)].map(|o| if o { rr } else { 0.0 })
                        }
                    };
                    // Connected modules fill their whole cell, so neighbours touch.
                    let r = if shape == Modules::Connected { c } else { r };
                    modules.push(round_rect(r, radii));
                }
            }
        }
    }
    // Finder patterns: a 7×7 ring around a 3×3 eye, in the three corners.
    let mut finders = Vec::new();
    for (fx, fy) in [(0, 0), (n - 7, 0), (0, n - 7)] {
        let outer = cell(fx, fy).union(cell(fx + 6, fy + 6));
        let inner = outer.inset(-m);
        let eye = outer.inset(-2.0 * m);
        let (ro, ri, re) = match style.finder {
            Finder::Square => (0.0, 0.0, 0.0),
            Finder::Rounded => (2.0 * m, 1.2 * m, 0.8 * m),
            Finder::Circle => (outer.width() / 2.0, inner.width() / 2.0, eye.width() / 2.0),
        };
        finders.push(round_rect(outer, [ro; 4]));
        let mut hole = round_rect(inner, [ri; 4]);
        hole.reverse();
        finders.push(hole);
        finders.push(round_rect(eye, [re; 4]));
    }
    let logo = clear.map(|(lo, hi)| cell(lo, lo).union(cell(hi - 1, hi - 1)).inset(-m / 2.0));
    let frame = Rect::from_center_size(r.center(), (side, side));
    Some(QrShapes { frame, modules: PathData::new(modules), finders: PathData::new(finders), logo, size: n })
}

/// The classic code's dark modules for `text` in `r`: one path, finder patterns included.
pub fn qr_path(text: &str, r: Rect) -> Option<PathData> {
    qr_styled_path(text, r, &QrStyle::default())
}

/// [`qr_shapes`] as one path: modules and finder patterns together.
pub fn qr_styled_path(text: &str, r: Rect, style: &QrStyle) -> Option<PathData> {
    let s = qr_shapes(text, r, style)?;
    let mut subpaths = s.finders.subpaths;
    subpaths.extend(s.modules.subpaths);
    Some(PathData::new(subpaths))
}

/// The modules per side for `text` at `ecc` (the version's size), if it can be encoded.
pub fn qr_size_at(text: &str, ecc: Ecc) -> Option<i32> {
    qrcodegen::QrCode::encode_text(text, ecc.level()).ok().map(|q| q.size())
}

/// The modules per side for `text` at the default error correction.
pub fn qr_size(text: &str) -> Option<i32> {
    qr_size_at(text, Ecc::default())
}

/// A clockwise rectangle with corner radii (top left, top right, bottom right, bottom left), each
/// at most half the shorter side.
fn round_rect(r: Rect, radii: [f64; 4]) -> SubPath {
    // The handle length that makes a cubic quarter circle.
    const K: f64 = 0.552_284_749_830_793_4;
    let max = (r.width().min(r.height()) / 2.0).max(0.0);
    let [tl, tr, br, bl] = radii.map(|v| v.clamp(0.0, max));
    let mut a = Vec::with_capacity(8);
    let mut corner = |p: Point, rad: f64, from: kurbo::Vec2, to: kurbo::Vec2| {
        if rad <= 0.0 {
            a.push(Anchor::corner(p));
            return;
        }
        // Into the corner along `from`, out of it along `to`.
        let start = p - from * rad;
        let end = p + to * rad;
        a.push(Anchor::with_handles(start, start, start + from * rad * K));
        a.push(Anchor::with_handles(end, end - to * rad * K, end));
    };
    let (x, y) = (kurbo::Vec2::new(1.0, 0.0), kurbo::Vec2::new(0.0, 1.0));
    corner(Point::new(r.x0, r.y0), tl, -y, x);
    corner(Point::new(r.x1, r.y0), tr, x, y);
    corner(Point::new(r.x1, r.y1), br, y, -x);
    corner(Point::new(r.x0, r.y1), bl, -x, -y);
    SubPath::new(a, true)
}

#[cfg(test)]
mod tests {
    use kurbo::Shape;

    use super::*;

    #[test]
    fn qr_modules_fill_the_square() {
        let p = qr_path("https://example.com", Rect::new(0.0, 0.0, 100.0, 100.0)).unwrap();
        let n = qr_size("https://example.com").unwrap();
        assert!(n >= 21 && (n - 17) % 4 == 0, "a QR version size: {n}");
        let b = p.bounds().unwrap();
        let m = 100.0 / (n + 4) as f64;
        // Finder patterns put dark modules at the corners of the symbol.
        assert!((b.x0 - 2.0 * m).abs() < 1e-9 && (b.y0 - 2.0 * m).abs() < 1e-9);
        assert!((b.x1 - (100.0 - 2.0 * m)).abs() < 1e-9);
        assert!(p.subpaths.len() > 20);
        assert!(qr_path(&"x".repeat(5000), Rect::new(0.0, 0.0, 10.0, 10.0)).is_none());
    }

    /// The code drawn as a scanner sees it: `px` pixels a side, dark where the path covers a
    /// pixel's centre (nonzero winding), light quiet zone and background.
    fn raster(p: &PathData, side: f64, px: usize) -> (usize, Vec<u8>) {
        // Each subpath with its bounds: a pixel only asks the few that can cover it.
        let parts: Vec<(Rect, kurbo::BezPath)> = p
            .subpaths
            .iter()
            .map(|sp| {
                let mut bp = kurbo::BezPath::new();
                sp.to_bezpath_into(&mut bp);
                (bp.bounding_box(), bp)
            })
            .collect();
        let k = side / px as f64;
        let mut img = vec![255u8; px * px];
        for (i, v) in img.iter_mut().enumerate() {
            let pt = Point::new(((i % px) as f64 + 0.5) * k, ((i / px) as f64 + 0.5) * k);
            let w: i32 = parts.iter().filter(|(b, _)| b.contains(pt)).map(|(_, bp)| bp.winding(pt)).sum();
            if w != 0 {
                *v = 0;
            }
        }
        (px, img)
    }

    /// The code for `text` drawn in `style`, eight pixels a module (as a phone camera sees a
    /// printed code at a short distance).
    fn image(text: &str, style: &QrStyle) -> (usize, Vec<u8>) {
        let side = 100.0;
        let p = qr_styled_path(text, Rect::new(0.0, 0.0, side, side), style).unwrap();
        let n = qr_size_at(text, style.ecc).unwrap() as usize;
        raster(&p, side, (n + 4) * 8)
    }

    /// What rqrr, an open-source QR reader, reads.
    fn read_rqrr(text: &str, style: &QrStyle) -> Option<String> {
        let (w, img) = image(text, style);
        let mut prepared = rqrr::PreparedImage::prepare_from_greyscale(w, w, |x, y| img[y * w + x]);
        let grids = prepared.detect_grids();
        grids.first()?.decode().ok().map(|(_, content)| content)
    }

    /// What ZXing (its Rust port rxing) reads: the way most phone scanners read.
    fn read_zxing(text: &str, style: &QrStyle) -> Option<String> {
        let (w, img) = image(text, style);
        rxing::helpers::detect_in_luma(img, w as u32, w as u32, Some(rxing::BarcodeFormat::QR_CODE)).ok().map(|r| r.getText().to_string())
    }

    const URL: &str = "https://github.com/storytold/designcraft/issues?q=qr";

    /// Every module shape and finder shape reads with ZXing, at every error correction level,
    /// with the modules at their usual size and noticeably apart. rqrr finds finder patterns by
    /// their corners: it reads every module shape with square finder patterns, not round ones.
    #[test]
    fn every_style_reads_back() {
        let mut failed = Vec::new();
        for ecc in [Ecc::Low, Ecc::Medium, Ecc::Quartile, Ecc::High] {
            for modules in [Modules::Square, Modules::Rounded, Modules::Dots, Modules::Connected] {
                for finder in [Finder::Square, Finder::Rounded, Finder::Circle] {
                    for scale in [1.0, 0.8] {
                        let style = QrStyle { ecc, modules, finder, scale, roundness: 1.0, logo: 0.0 };
                        if read_zxing(URL, &style).as_deref() != Some(URL) {
                            failed.push(format!("zxing {ecc:?}/{modules:?}/{finder:?}/{scale}"));
                        }
                        if finder == Finder::Square && read_rqrr(URL, &style).as_deref() != Some(URL) {
                            failed.push(format!("rqrr {ecc:?}/{modules:?}/{finder:?}/{scale}"));
                        }
                    }
                }
            }
        }
        assert!(failed.is_empty(), "unreadable: {failed:?}");
    }

    /// A logo area as large as the dialog allows reads at High error correction, in every module
    /// and finder shape; at Low the same area breaks the code (the error correction pays for it).
    #[test]
    fn a_logo_area_reads_at_high_error_correction() {
        let mut failed = Vec::new();
        for modules in [Modules::Square, Modules::Rounded, Modules::Dots, Modules::Connected] {
            for finder in [Finder::Square, Finder::Rounded, Finder::Circle] {
                let style = QrStyle { ecc: Ecc::High, modules, finder, logo: MAX_LOGO, ..Default::default() };
                if read_zxing(URL, &style).as_deref() != Some(URL) {
                    failed.push(format!("zxing {modules:?}/{finder:?}"));
                }
                if finder == Finder::Square && read_rqrr(URL, &style).as_deref() != Some(URL) {
                    failed.push(format!("rqrr {modules:?}/{finder:?}"));
                }
            }
        }
        assert!(failed.is_empty(), "unreadable with a logo: {failed:?}");
        let low = QrStyle { ecc: Ecc::Low, logo: MAX_LOGO, ..Default::default() };
        assert_eq!(read_zxing(URL, &low), None, "a third of the side cleared at Low error correction");
    }

    #[test]
    fn the_logo_area_is_centred_and_clear() {
        let style = QrStyle { ecc: Ecc::High, logo: 0.3, ..Default::default() };
        let s = qr_shapes(URL, Rect::new(0.0, 0.0, 100.0, 100.0), &style).unwrap();
        let logo = s.logo.unwrap();
        assert!((logo.center() - Point::new(50.0, 50.0)).hypot() < 1e-9);
        assert!(logo.width() > 25.0 && logo.width() < 32.0, "{logo:?}");
        let bp = s.modules.to_bezpath();
        assert_eq!(bp.winding(logo.center()), 0, "nothing drawn where the logo goes");
        assert!(QrStyle::default().is_classic() && !style.is_classic());
    }

    #[test]
    fn finder_rings_have_a_hole() {
        let s = qr_shapes(URL, Rect::new(0.0, 0.0, 100.0, 100.0), &QrStyle { finder: Finder::Circle, ..Default::default() }).unwrap();
        let m = 100.0 / (s.size + 4) as f64;
        let bp = s.finders.to_bezpath();
        // Top left finder: ring at 0.5 module in, light between ring and eye, dark eye centre.
        let at = |modules: f64| Point::new(2.0 * m + modules * m, 2.0 * m + 3.5 * m);
        assert_ne!(bp.winding(at(0.5)), 0, "the ring");
        assert_eq!(bp.winding(at(1.5)), 0, "between ring and eye");
        assert_ne!(bp.winding(at(3.5)), 0, "the eye");
    }

    #[test]
    fn hostile_numbers_are_clamped() {
        let s = QrStyle { roundness: f64::NAN, scale: 9.0, logo: -1.0, ..Default::default() }.sanitized();
        assert_eq!((s.roundness, s.scale, s.logo), (1.0, 1.0, 0.0));
    }

    /// A rectangle given corner to corner the other way round makes the same code (it panicked
    /// in `round_rect` for rounded modules); an empty or non-finite one makes none.
    #[test]
    fn a_reversed_rectangle_makes_the_same_code() {
        for modules in [Modules::Square, Modules::Rounded, Modules::Dots, Modules::Connected] {
            let style = QrStyle { modules, scale: 0.8, finder: Finder::Rounded, ..Default::default() };
            let fwd = qr_shapes("https://example.com", Rect::new(100.0, 100.0, 200.0, 200.0), &style).unwrap();
            let back = qr_shapes("https://example.com", Rect::new(200.0, 200.0, 100.0, 100.0), &style).unwrap();
            assert_eq!(fwd, back, "{modules:?}");
        }
        for r in [
            Rect::new(5.0, 5.0, 5.0, 50.0),
            Rect::new(0.0, 0.0, f64::NAN, 9.0),
            Rect::new(0.0, 0.0, f64::INFINITY, f64::INFINITY),
            Rect::new(0.0, 0.0, 10.0, f64::INFINITY),
        ] {
            assert!(qr_shapes("x", r, &QrStyle::default()).is_none(), "{r:?}");
        }
    }
}
