//! Convert a PDF page into editable path and image art by interpreting its drawing operators
//! (Hayro via craft-pdf). Glyphs become outlines; patterns and soft masks are approximated or
//! skipped with warnings. Paint order is preserved so objects stack as they did in the PDF.

use craft_pdf::interpret::font::Glyph;
use craft_pdf::interpret::{
    BlendMode, ClipPath, Context, Device, FillRule, GlyphDrawMode, Image, ImageData, InterpreterCache, InterpreterSettings, Paint, PathDrawMode,
    SoftMask, TransformExt, interpret_page,
};
use designcraft_geom::{Affine, BezPath, Point};
use vello_cpu::kurbo;

/// Soft cap so a hostile or extremely dense PDF cannot allocate without bound.
pub const MAX_PDF_ART_OBJECTS: usize = 50_000;

/// Soft cap on decoded raster pixels kept for one page's images.
pub const MAX_PDF_ART_IMAGE_PIXELS: u64 = 16_000_000;

/// Solid paint taken from a PDF colour. Pattern paints are skipped (not converted).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PdfArtPaint {
    pub rgb: [u8; 3],
    pub opacity: f32,
}

/// Stroke style in page points.
#[derive(Clone, Debug, PartialEq)]
pub struct PdfArtStroke {
    pub width: f64,
    pub cap: PdfArtCap,
    pub join: PdfArtJoin,
    pub miter: f64,
    pub dashes: Option<Vec<f64>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PdfArtCap {
    Butt,
    Round,
    Square,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PdfArtJoin {
    Miter,
    Round,
    Bevel,
}

/// One filled and/or stroked path in page space (points, y down as DesignCraft uses).
#[derive(Clone, Debug)]
pub struct PdfArtPath {
    pub path: BezPath,
    pub fill: Option<PdfArtPaint>,
    pub stroke: Option<(PdfArtPaint, PdfArtStroke)>,
    pub opacity: f32,
    pub even_odd: bool,
}

/// A raster image drawn on the page: PNG bytes and the transform that maps the image's pixel
/// rectangle `(0,0)–(width,height)` into page space.
#[derive(Clone, Debug)]
pub struct PdfArtImage {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub xf: Affine,
}

/// One drawing in PDF paint order (first = back, last = front).
#[derive(Clone, Debug)]
pub enum PdfArtOp {
    Path(PdfArtPath),
    Image(PdfArtImage),
}

/// Editable art from one PDF page.
#[derive(Clone, Debug, Default)]
pub struct PdfPageArt {
    pub width: f64,
    pub height: f64,
    /// Drawing operators in the order the PDF painted them (back to front).
    pub ops: Vec<PdfArtOp>,
    pub warnings: Vec<String>,
}

impl PdfPageArt {
    /// Paths only, in paint order (for callers that still iterate by kind).
    pub fn paths(&self) -> impl Iterator<Item = &PdfArtPath> {
        self.ops.iter().filter_map(|op| match op {
            PdfArtOp::Path(p) => Some(p),
            PdfArtOp::Image(_) => None,
        })
    }

    /// Images only, in paint order.
    pub fn images(&self) -> impl Iterator<Item = &PdfArtImage> {
        self.ops.iter().filter_map(|op| match op {
            PdfArtOp::Image(i) => Some(i),
            PdfArtOp::Path(_) => None,
        })
    }
}

/// Why a PDF page could not be converted.
#[derive(Debug, thiserror::Error)]
pub enum PdfConvertError {
    #[error("{0}")]
    Input(#[source] craft_pdf::OpenError),
    #[error("the PDF page could not be read")]
    InvalidPage,
    #[error("the PDF has no pages")]
    Empty,
}

/// Shared object / image-pixel budget across pages of one PDF open.
#[derive(Clone, Debug)]
struct PdfArtBudget {
    objects: usize,
    image_pixels: u64,
    capped: bool,
}

impl PdfArtBudget {
    fn fresh() -> Self {
        Self { objects: 0, image_pixels: 0, capped: false }
    }
}

/// Convert PDF page `page` (0-based) into editable paths and images.
pub fn pdf_page_art(bytes: &[u8], page: usize) -> Result<PdfPageArt, PdfConvertError> {
    pdf_page_art_with_budget(bytes, page, &mut PdfArtBudget::fresh())
}

fn pdf_page_art_with_budget(bytes: &[u8], page: usize, budget: &mut PdfArtBudget) -> Result<PdfPageArt, PdfConvertError> {
    check_pdf_input_size(bytes.len()).map_err(|e| match e {
        super::PdfRasterError::Input(err) => PdfConvertError::Input(err),
        _ => PdfConvertError::InvalidPage,
    })?;
    let pdf = craft_pdf::open_slice(bytes, None).map_err(PdfConvertError::Input)?;
    let p = pdf.pages().get(page).ok_or(PdfConvertError::InvalidPage)?;
    let (w, h) = p.render_dimensions();
    let (w, h) = (f64::from(w), f64::from(h));
    if !(w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite()) {
        return Err(PdfConvertError::InvalidPage);
    }
    let initial = p.initial_transform(true).to_kurbo();
    let page_rect = kurbo::Rect::new(0.0, 0.0, w, h);
    let cache = InterpreterCache::new();
    let mut ctx = Context::new(initial, page_rect, &cache, p.xref(), InterpreterSettings::default());
    let mut art = Collector { ops: Vec::new(), warnings: Notes::default(), hidden: Vec::new(), budget };
    interpret_page(p, &mut ctx, &mut art);
    let capped = art.budget.capped;
    let mut warnings = art.warnings.into_warnings();
    if capped {
        warnings.push(format!("stopped after {MAX_PDF_ART_OBJECTS} objects (further drawing left out)"));
    }
    Ok(PdfPageArt { width: w, height: h, ops: art.ops, warnings })
}

/// Convert every page of a PDF. An empty file is an error.
/// Object and image-pixel caps are shared across the whole document.
pub fn pdf_document_art(bytes: &[u8]) -> Result<Vec<PdfPageArt>, PdfConvertError> {
    let count = super::pdf_page_count(bytes).ok_or(PdfConvertError::InvalidPage)?;
    if count == 0 {
        return Err(PdfConvertError::Empty);
    }
    let mut budget = PdfArtBudget::fresh();
    let mut pages: Vec<PdfPageArt> = Vec::new();
    for i in 0..count {
        if budget.capped {
            let mut skipped = PdfPageArt::default();
            // Keep page size from the previous page when possible so open still builds a sheet.
            if let Some(prev) = pages.last() {
                skipped.width = prev.width;
                skipped.height = prev.height;
            }
            skipped.warnings.push(format!("page {}: left out (document conversion budget exhausted)", i + 1));
            pages.push(skipped);
            continue;
        }
        pages.push(pdf_page_art_with_budget(bytes, i, &mut budget)?);
    }
    Ok(pages)
}

use super::check_pdf_input_size;

#[derive(Default)]
struct Notes {
    patterns: usize,
    soft_masks: usize,
    stencils: usize,
    images_skipped: usize,
    invisible: usize,
}

impl Notes {
    fn into_warnings(self) -> Vec<String> {
        let mut w = Vec::new();
        if self.patterns > 0 {
            w.push(format!("{} pattern paint(s) skipped (paths using them left out)", self.patterns));
        }
        if self.soft_masks > 0 {
            w.push(format!("{} soft mask(s) ignored", self.soft_masks));
        }
        if self.stencils > 0 {
            w.push(format!("{} stencil image(s) left out", self.stencils));
        }
        if self.images_skipped > 0 {
            w.push(format!("{} image(s) left out (decode failed or size limit)", self.images_skipped));
        }
        if self.invisible > 0 {
            w.push(format!("{} invisible text run(s) left out", self.invisible));
        }
        w
    }
}

struct Collector<'b> {
    ops: Vec<PdfArtOp>,
    warnings: Notes,
    hidden: Vec<bool>,
    budget: &'b mut PdfArtBudget,
}

impl Collector<'_> {
    fn room(&mut self) -> bool {
        if self.budget.capped {
            return false;
        }
        if self.budget.objects >= MAX_PDF_ART_OBJECTS {
            self.budget.capped = true;
            return false;
        }
        true
    }

    fn hidden(&self) -> bool {
        self.hidden.last().copied().unwrap_or(false)
    }

    fn paint(&mut self, paint: &Paint<'_>) -> Option<PdfArtPaint> {
        match paint {
            Paint::Color(c) => {
                let [r, g, b, a] = c.to_rgba().to_rgba8();
                if a == 0 {
                    return None;
                }
                Some(PdfArtPaint { rgb: [r, g, b], opacity: f32::from(a) / 255.0 })
            }
            Paint::Pattern(_) => {
                self.warnings.patterns += 1;
                // No reliable solid colour without interpreting the pattern; skip the paint.
                None
            }
        }
    }

    fn stroke_style(s: &craft_pdf::interpret::StrokeProps, transform: kurbo::Affine) -> PdfArtStroke {
        let scale = transform.determinant().abs().sqrt();
        let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
        // PDF width 0 is a device hairline; the engine hides weight <= 0, so keep a visible minimum.
        let width = round(f64::from(s.line_width.max(0.0)) * scale).max(0.25);
        let dashes = if s.dash_array.is_empty() { None } else { Some(s.dash_array.iter().map(|v| round(f64::from(*v) * scale)).collect()) };
        PdfArtStroke {
            width,
            cap: match s.line_cap {
                kurbo::Cap::Butt => PdfArtCap::Butt,
                kurbo::Cap::Round => PdfArtCap::Round,
                kurbo::Cap::Square => PdfArtCap::Square,
            },
            join: match s.line_join {
                kurbo::Join::Round => PdfArtJoin::Round,
                kurbo::Join::Bevel => PdfArtJoin::Bevel,
                _ => PdfArtJoin::Miter,
            },
            miter: f64::from(s.miter_limit),
            dashes,
        }
    }

    fn push_path(&mut self, path: &kurbo::BezPath, transform: kurbo::Affine, paint: &Paint<'_>, mode: &PathDrawMode) {
        if self.hidden() || !self.room() {
            return;
        }
        let xf = to_affine(transform);
        let mapped = map_path(path, xf);
        if mapped.elements().is_empty() {
            return;
        }
        let (fill, stroke, even_odd) = match mode {
            PathDrawMode::Fill(rule) => {
                let Some(p) = self.paint(paint) else { return };
                (Some(p), None, matches!(rule, FillRule::EvenOdd))
            }
            PathDrawMode::Stroke(s) => {
                let Some(p) = self.paint(paint) else { return };
                (None, Some((p, Self::stroke_style(s, transform))), false)
            }
        };
        self.ops.push(PdfArtOp::Path(PdfArtPath { path: mapped, fill, stroke, opacity: 1.0, even_odd }));
        self.budget.objects = self.budget.objects.saturating_add(1);
    }
}

impl<'a> Device<'a> for Collector<'_> {
    fn set_soft_mask(&mut self, mask: Option<SoftMask<'a>>) {
        if mask.is_some() {
            self.warnings.soft_masks += 1;
        }
    }

    fn set_blend_mode(&mut self, _: BlendMode) {}

    fn draw_path(&mut self, path: &kurbo::BezPath, transform: kurbo::Affine, paint: &Paint<'a>, mode: &PathDrawMode) {
        self.push_path(path, transform, paint, mode);
    }

    fn push_clip_path(&mut self, _: &ClipPath) {
        // Clip is not modelled on DesignCraft items yet; geometry is still captured unclipped.
    }

    fn push_transparency_group(&mut self, opacity: f32, mask: Option<SoftMask<'a>>, _: BlendMode) {
        if mask.is_some() {
            self.warnings.soft_masks += 1;
        }
        let parent = self.hidden();
        self.hidden.push(parent || opacity <= 0.0);
    }

    fn draw_glyph(&mut self, glyph: &Glyph<'a>, transform: kurbo::Affine, glyph_transform: kurbo::Affine, paint: &Paint<'a>, mode: &GlyphDrawMode) {
        let path_mode = match mode {
            GlyphDrawMode::Invisible => {
                self.warnings.invisible += 1;
                return;
            }
            GlyphDrawMode::Clip => return,
            GlyphDrawMode::Fill => PathDrawMode::Fill(FillRule::NonZero),
            GlyphDrawMode::Stroke(s) => PathDrawMode::Stroke(s.clone()),
        };
        match glyph {
            Glyph::Outline(o) => self.push_path(&o.outline(), transform * glyph_transform, paint, &path_mode),
            Glyph::Type3(g) => g.interpret(self, transform, glyph_transform, paint),
        }
    }

    fn draw_image(&mut self, image: Image<'a, '_>, transform: kurbo::Affine) {
        if self.hidden() || !self.room() {
            return;
        }
        match image {
            Image::Stencil(_) => {
                self.warnings.stencils += 1;
            }
            Image::Raster(r) => {
                let mut captured = None;
                let mut saw = false;
                r.with_rgba(
                    |data, alpha| {
                        saw = true;
                        let (w, h) = (data.width(), data.height());
                        if w == 0 || h == 0 {
                            self.warnings.images_skipped += 1;
                            return;
                        }
                        let pixels = u64::from(w).saturating_mul(u64::from(h));
                        if self.budget.image_pixels.saturating_add(pixels) > MAX_PDF_ART_IMAGE_PIXELS {
                            self.warnings.images_skipped += 1;
                            return;
                        }
                        let rgba = match rgba_bytes(&data, alpha.as_ref()) {
                            Some(px) => px,
                            None => {
                                self.warnings.images_skipped += 1;
                                return;
                            }
                        };
                        let png = encode_png(w, h, &rgba);
                        if png.is_empty() {
                            self.warnings.images_skipped += 1;
                            return;
                        }
                        self.budget.image_pixels = self.budget.image_pixels.saturating_add(pixels);
                        let (sx, sy) = data.scale_factors();
                        let scale = Affine::scale_non_uniform(f64::from(sx), f64::from(sy));
                        let xf = to_affine(transform) * scale;
                        captured = Some(PdfArtImage { png, width: w, height: h, xf });
                    },
                    None,
                );
                if let Some(img) = captured {
                    self.ops.push(PdfArtOp::Image(img));
                    self.budget.objects = self.budget.objects.saturating_add(1);
                } else if !saw {
                    self.warnings.images_skipped += 1;
                }
            }
        }
    }

    fn pop_clip_path(&mut self) {}

    fn pop_transparency_group(&mut self) {
        self.hidden.pop();
    }
}

fn rgba_bytes(data: &ImageData, alpha: Option<&craft_pdf::interpret::LumaData>) -> Option<Vec<u8>> {
    let (w, h) = (data.width(), data.height());
    let n = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
    let mut out = vec![0u8; n];
    match data {
        ImageData::Rgb(rgb) => {
            if rgb.data.len() < (w as usize).saturating_mul(h as usize).saturating_mul(3) {
                return None;
            }
            for i in 0..(w as usize * h as usize) {
                let o = i * 4;
                let s = i * 3;
                out[o] = rgb.data[s];
                out[o + 1] = rgb.data[s + 1];
                out[o + 2] = rgb.data[s + 2];
                out[o + 3] = 255;
            }
        }
        ImageData::Luma(luma) => {
            if luma.data.len() < (w as usize).saturating_mul(h as usize) {
                return None;
            }
            for i in 0..(w as usize * h as usize) {
                let v = luma.data[i];
                let o = i * 4;
                out[o] = v;
                out[o + 1] = v;
                out[o + 2] = v;
                out[o + 3] = 255;
            }
        }
    }
    if let Some(a) = alpha
        && a.width == w
        && a.height == h
        && a.data.len() >= (w as usize).saturating_mul(h as usize)
    {
        for i in 0..(w as usize * h as usize) {
            out[i * 4 + 3] = a.data[i];
        }
    }
    Some(out)
}

fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let Some(img) = image::RgbaImage::from_raw(width, height, rgba.to_vec()) else {
        return Vec::new();
    };
    let mut buf = Vec::new();
    if image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png).is_err() {
        return Vec::new();
    }
    buf
}

fn round(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

fn to_affine(a: kurbo::Affine) -> Affine {
    Affine::new(a.as_coeffs())
}

fn map_path(path: &kurbo::BezPath, xf: Affine) -> BezPath {
    let mut out = BezPath::new();
    for el in path.elements() {
        match *el {
            kurbo::PathEl::MoveTo(p) => {
                let q = xf * Point::new(p.x, p.y);
                out.move_to(q);
            }
            kurbo::PathEl::LineTo(p) => {
                let q = xf * Point::new(p.x, p.y);
                out.line_to(q);
            }
            kurbo::PathEl::QuadTo(p1, p2) => {
                let a = xf * Point::new(p1.x, p1.y);
                let b = xf * Point::new(p2.x, p2.y);
                out.quad_to(a, b);
            }
            kurbo::PathEl::CurveTo(p1, p2, p3) => {
                let a = xf * Point::new(p1.x, p1.y);
                let b = xf * Point::new(p2.x, p2.y);
                let c = xf * Point::new(p3.x, p3.y);
                out.curve_to(a, b, c);
            }
            kurbo::PathEl::ClosePath => out.close_path(),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_geom::Shape;

    fn pdf(page: &str, content: &str) -> Vec<u8> {
        let bodies = [
            b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
            b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
            format!("<</Type/Page/Parent 2 0 R{page}/Contents 4 0 R>>").into_bytes(),
            {
                let mut s = format!("<< /Length {}>>\nstream\n", content.len()).into_bytes();
                s.extend_from_slice(content.as_bytes());
                s.extend_from_slice(b"\nendstream");
                s
            },
        ];
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in bodies.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f\r\n", bodies.len() + 1).as_bytes());
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n\r\n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<</Size {}/Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n", bodies.len() + 1).as_bytes());
        out
    }

    #[test]
    fn a_filled_rectangle_becomes_an_editable_path() {
        let bytes = pdf("/MediaBox[0 0 200 100]", "1 0 0 rg 10 20 30 40 re f");
        let art = pdf_page_art(&bytes, 0).unwrap();
        assert!((art.width - 200.0).abs() < 0.01);
        assert!((art.height - 100.0).abs() < 0.01);
        assert_eq!(art.ops.len(), 1, "{art:?}");
        let PdfArtOp::Path(p) = &art.ops[0] else { panic!("expected path") };
        assert_eq!(p.fill.map(|f| f.rgb), Some([255, 0, 0]));
        // PDF y-up → page y-down: MediaBox height 100, re at (10,20) size 30×40 → y 40..80.
        let b = p.path.bounding_box();
        assert!((b.x0 - 10.0).abs() < 0.5 && (b.y0 - 40.0).abs() < 0.5, "{b:?}");
        assert!((b.width() - 30.0).abs() < 0.5 && (b.height() - 40.0).abs() < 0.5, "{b:?}");
    }

    #[test]
    fn paint_order_keeps_image_before_later_path() {
        // A 1×1 DeviceRGB image, then a green fill on top — image must stay behind the path.
        let mut stream = b"q 10 0 0 10 0 0 cm\nBI /W 1 /H 1 /CS /RGB /BPC 8 /L 3 ID ".to_vec();
        stream.extend_from_slice(&[0xFF, 0x00, 0x00]);
        stream.extend_from_slice(b"\nEI\nQ\n0 1 0 rg 0 0 20 20 re f");
        let bodies = [
            b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
            b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
            b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 20 20]/Contents 4 0 R>>".to_vec(),
            {
                let mut s = format!("<< /Length {}>>\nstream\n", stream.len()).into_bytes();
                s.extend_from_slice(&stream);
                s.extend_from_slice(b"\nendstream");
                s
            },
        ];
        let mut bytes = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in bodies.iter().enumerate() {
            offsets.push(bytes.len());
            bytes.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            bytes.extend_from_slice(body);
            bytes.extend_from_slice(b"\nendobj\n");
        }
        let xref = bytes.len();
        bytes.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f\r\n", bodies.len() + 1).as_bytes());
        for offset in offsets {
            bytes.extend_from_slice(format!("{offset:010} 00000 n\r\n").as_bytes());
        }
        bytes.extend_from_slice(format!("trailer\n<</Size {}/Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n", bodies.len() + 1).as_bytes());
        let art = pdf_page_art(&bytes, 0).unwrap();
        assert!(art.ops.len() >= 2, "ops={}", art.ops.len());
        let mut saw_image = false;
        let mut path_after_image = false;
        for op in &art.ops {
            match op {
                PdfArtOp::Image(_) => saw_image = true,
                PdfArtOp::Path(_) if saw_image => path_after_image = true,
                PdfArtOp::Path(_) => {}
            }
        }
        assert!(saw_image, "expected a raster image in the page");
        assert!(path_after_image, "path that paints after the image must stay after it");
    }

    #[test]
    fn missing_page_is_an_error() {
        let bytes = pdf("/MediaBox[0 0 10 10]", "");
        assert!(pdf_page_art(&bytes, 1).is_err());
    }

    #[test]
    fn empty_drawing_yields_no_ops() {
        let bytes = pdf("/MediaBox[0 0 50 50]", "");
        let art = pdf_page_art(&bytes, 0).unwrap();
        assert!(art.ops.is_empty());
    }

    #[test]
    fn even_odd_compound_path_keeps_its_hole() {
        // Outer rectangle with an inner hole (even-odd): the hole must stay even_odd=true.
        let bytes = pdf("/MediaBox[0 0 100 100]", "1 1 1 rg 0 0 100 100 re 20 20 60 60 re f*");
        let art = pdf_page_art(&bytes, 0).unwrap();
        assert_eq!(art.ops.len(), 1);
        let PdfArtOp::Path(p) = &art.ops[0] else { panic!("path") };
        assert!(p.even_odd, "f* must set even-odd");
        assert_eq!(p.fill.map(|f| f.rgb), Some([255, 255, 255]));
    }
}
