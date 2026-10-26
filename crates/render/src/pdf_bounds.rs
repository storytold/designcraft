//! The bounds of what a PDF page draws, found by interpreting its drawing operations: the union
//! of every painted path, glyph and image, each cut to the clip in force when it's painted.
//! Nothing is rasterized, so measuring a page costs a fraction of drawing it.

use craft_pdf::interpret::font::Glyph;
use craft_pdf::interpret::{
    BlendMode, ClipPath, Context, Device, FillRule, GlyphDrawMode, Image, InterpreterCache, InterpreterSettings, Paint, PathDrawMode, RectExt,
    SoftMask, TransformExt, interpret_page,
};
use vello_cpu::kurbo::{self, Affine, BezPath, Rect, Shape};

/// The bounds of what PDF page `page` draws, as fractions of the page as rendered (its visible
/// crop box, rotated as it displays): left, top, right, bottom. `None` when it draws nothing.
pub(crate) fn page_content_bounds(bytes: &[u8], page: usize) -> Option<[f64; 4]> {
    let pdf = craft_pdf::open_slice(bytes, None).ok()?;
    let p = pdf.pages().get(page)?;
    let (w, h) = p.render_dimensions();
    let (w, h) = (f64::from(w), f64::from(h));
    if !(w > 0.0 && h > 0.0) {
        return None;
    }
    let initial = p.initial_transform(true).to_kurbo();
    let page_rect = Rect::new(0.0, 0.0, w, h);
    let crop = (initial * p.intersected_crop_box().to_kurbo().to_path(0.1)).bounding_box().intersect(page_rect);
    let cache = InterpreterCache::new();
    let mut ctx = Context::new(initial, page_rect, &cache, p.xref(), InterpreterSettings::default());
    let mut dev = Bounds { clips: vec![crop], hidden: Vec::new(), out: None };
    interpret_page(p, &mut ctx, &mut dev);
    let r = dev.out?;
    let frac = [r.x0 / w, r.y0 / h, r.x1 / w, r.y1 / h];
    frac.iter().all(|v| v.is_finite()).then_some(frac)
}

/// A [`Device`] that collects bounds instead of drawing.
struct Bounds {
    /// The bounding box of each clip in force, innermost last (device space); the first is the
    /// page's crop box.
    clips: Vec<Rect>,
    /// For each open transparency group: whether nothing in it can show (opacity 0).
    hidden: Vec<bool>,
    out: Option<Rect>,
}

impl Bounds {
    fn add(&mut self, r: Rect) {
        if self.hidden.last().copied().unwrap_or(false) {
            return;
        }
        let Some(clip) = self.clips.last() else { return };
        let r = r.intersect(*clip);
        if !(r.width() > 0.0 && r.height() > 0.0 && [r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite())) {
            return;
        }
        self.out = Some(self.out.map_or(r, |o| o.union(r)));
    }

    fn add_path(&mut self, path: &BezPath, transform: Affine, paint: &Paint<'_>, mode: &PathDrawMode) {
        if let Paint::Color(c) = paint
            && c.to_rgba().to_rgba8()[3] == 0
        {
            return;
        }
        let r = match mode {
            PathDrawMode::Fill(_) => (transform * path.clone()).bounding_box(),
            PathDrawMode::Stroke(s) => {
                // Dashes only remove parts of the line: the undashed stroke bounds them.
                let stroke = kurbo::Stroke {
                    width: f64::from(s.line_width.max(0.0)),
                    join: s.line_join,
                    miter_limit: f64::from(s.miter_limit),
                    start_cap: s.line_cap,
                    end_cap: s.line_cap,
                    dash_pattern: Default::default(),
                    dash_offset: 0.0,
                };
                // A quarter point on the page, in the path's own units.
                let scale = transform.determinant().abs().sqrt();
                let tolerance = if scale.is_finite() && scale > 1e-9 { (0.25 / scale).min(1e6) } else { 0.25 };
                let outline = kurbo::stroke(path.iter(), &stroke, &kurbo::StrokeOpts::default(), tolerance);
                (transform * outline).bounding_box()
            }
        };
        self.add(r);
    }
}

impl<'a> Device<'a> for Bounds {
    fn set_soft_mask(&mut self, _: Option<SoftMask<'a>>) {}

    fn set_blend_mode(&mut self, _: BlendMode) {}

    fn draw_path(&mut self, path: &BezPath, transform: Affine, paint: &Paint<'a>, mode: &PathDrawMode) {
        self.add_path(path, transform, paint, mode);
    }

    fn push_clip_path(&mut self, clip: &ClipPath) {
        let top = self.clips.last().copied().unwrap_or(Rect::ZERO);
        self.clips.push(clip.path.bounding_box().intersect(top));
    }

    fn push_transparency_group(&mut self, opacity: f32, _: Option<SoftMask<'a>>, _: BlendMode) {
        let parent = self.hidden.last().copied().unwrap_or(false);
        self.hidden.push(parent || opacity <= 0.0);
    }

    fn draw_glyph(&mut self, glyph: &Glyph<'a>, transform: Affine, glyph_transform: Affine, paint: &Paint<'a>, mode: &GlyphDrawMode) {
        let mode = match mode {
            // Clipping text paints nothing; its clip arrives through `push_clip_path`.
            GlyphDrawMode::Invisible | GlyphDrawMode::Clip => return,
            GlyphDrawMode::Fill => PathDrawMode::Fill(FillRule::NonZero),
            GlyphDrawMode::Stroke(s) => PathDrawMode::Stroke(s.clone()),
        };
        match glyph {
            Glyph::Outline(o) => self.add_path(&o.outline(), transform * glyph_transform, paint, &mode),
            Glyph::Type3(g) => g.interpret(self, transform, glyph_transform, paint),
        }
    }

    fn draw_image(&mut self, image: Image<'a, '_>, transform: Affine) {
        let r = Rect::new(0.0, 0.0, f64::from(image.width()), f64::from(image.height()));
        self.add((transform * r.to_path(0.1)).bounding_box());
    }

    fn pop_clip_path(&mut self) {
        // The crop box stays.
        if self.clips.len() > 1 {
            self.clips.pop();
        }
    }

    fn pop_transparency_group(&mut self) {
        self.hidden.pop();
    }
}
