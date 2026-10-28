//! DesignCraft renderer: spreads → premultiplied RGBA pixels with `vello_cpu`.
//!
//! The renderer draws *document content only* (paper, page items, parent items, composed text,
//! placed images). Non-printing chrome — guides, frame edges, ports, selection — is drawn by the
//! UI as vector overlays so it stays crisp at any zoom.
//!
//! Callers pass a list of spreads with their placement on a shared canvas and a canvas → pixel
//! view transform, so the UI can render exactly its viewport with every visible spread.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use designcraft_color::BlendMode as DcBlend;
use designcraft_compose::{Cache, ComposedStory, ShownOn};
use designcraft_doc::{Content, Document, Item, PageSide, SpreadRef, StoryId, StrokeAlign, StrokeType};
use designcraft_geom::corners;
use designcraft_geom::{Affine, BezPath, Rect, Shape, Vec2};
use vello_cpu::kurbo;
use vello_cpu::peniko::{self, BlendMode, Compose, Mix};
use vello_cpu::{Pixmap, RenderContext, Resources};

pub use vello_cpu;

mod blendspace;
mod colorize;
pub mod damage;
mod fx;
pub mod glyphs;
mod grid;
mod pdf_bounds;
mod pdf_layers;
pub use pdf_layers::{pdf_hide_layers, pdf_layers};

mod color;
mod decode;
mod draw;
mod driver;
mod pdf_raster;
mod stroke;
mod types;

use color::{PLATE, rect_overlaps, set_fill_paint};
pub use color::{blend_mode, color_of, proof_view};
pub(crate) use color::{overprints, screen_color};
use decode::halve;
pub use decode::{decode_pixmap, decode_pixmap_page, image_mime, image_size, natural_size};
#[cfg(test)]
use pdf_raster::PDF_RENDERS;
pub(crate) use pdf_raster::pdf_raster_size;
pub use pdf_raster::{
    PdfRasterError, check_pdf_input_size, crop_pixmap, is_pdf, pdf_crop_box, pdf_page_box, pdf_page_count, pdf_page_png, pdf_page_png_checked,
    pdf_page_png_options_checked, pdf_page_size, render_pdf_page, shown_box,
};
pub(crate) use types::Frame;
pub use types::{
    Cancellation, DisplayQuality, DrawSlice, FrameStats, MAX_PAGE_PIXELS, MAX_SIDE, Placed, RenderOptions, Rendered, SlicePart, default_threads,
    effect_outset,
};
pub mod images;
mod text;

impl Rendered {
    pub fn to_straight(&self) -> Vec<u8> {
        let mut out = self.pixels.clone();
        for px in out.as_chunks_mut::<4>().0 {
            let a = px[3] as u32;
            if a != 0 && a != 255 {
                for c in &mut px[..3] {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        out
    }
    /// PNG bytes (empty, with the error logged, if the image can't be encoded).
    pub fn to_png(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut px = self.to_straight();
        // `pixels` is public: a mismatched buffer is padded or cut to the stated size.
        px.resize(self.width as usize * self.height as usize * 4, 0);
        let Some(img) = image::RgbaImage::from_raw(self.width, self.height, px) else {
            log::error!("PNG encode: {}×{} image is too large", self.width, self.height);
            return Vec::new();
        };
        if let Err(e) = img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png) {
            log::error!("PNG encode: {e}");
            return Vec::new();
        }
        buf
    }
    pub fn to_jpeg(&self, quality: u8) -> Vec<u8> {
        let rgba = self.to_straight();
        let rgb: Vec<u8> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| {
                let a = p[3] as u32;
                let mix = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
                [mix(p[0]), mix(p[1]), mix(p[2])]
            })
            .collect();
        let mut buf = Vec::new();
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality.clamp(1, 100));
        let _ = image::ImageEncoder::write_image(enc, &rgb, self.width, self.height, image::ExtendedColorType::Rgb8);
        buf
    }
    /// Straight-alpha RGBA at (x, y).
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        let p = &self.pixels[i..i + 4];
        let a = p[3] as u32;
        if a == 0 {
            return [0, 0, 0, 0];
        }
        let un = |c: u8| ((c as u32 * 255 + a / 2) / a).min(255) as u8;
        [un(p[0]), un(p[1]), un(p[2]), p[3]]
    }
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            background: None,
            paper: true,
            printing_only: false,
            hidden: vec![],
            slice: None,
            cancellation: None,
            greek_below_px: 0.0,
            page_shadow: false,
            highlight_missing_fonts: false,
            highlight_hj: false,
            highlight_keeps: false,
            highlight_custom_tracking: false,
            condition_indicators: false,
            note_indicators: false,
            change_markup: false,
            tag_markers: false,
            plate: None,
            quality: DisplayQuality::High,
            rich_black: false,
            overprint_preview: false,
            blend_space_view: false,
            document_grid: vec![],
        }
    }
}

/// Reusable renderer: keeps the render context, the frame-space glyph paths of composed text
/// and (process-wide) decoded images between renders.
pub struct Renderer {
    ctx: Option<RenderContext>,
    resources: Resources,
    glyphs: text::GlyphCache,
    /// Composed stories looked up during the current render.
    stories: HashMap<(StoryId, ShownOn), Arc<ComposedStory>>,
    drawing_items: Vec<designcraft_doc::ItemId>,
    cmyk_view: Option<blendspace::CachedView>,
    /// Whether the last render ran the CMYK ink passes (only then is its view worth keeping).
    cmyk_ran: bool,
    #[cfg(test)]
    cmyk_passes: usize,
    /// Pixels the last ink pass covered (tests).
    #[cfg(test)]
    cmyk_pass_pixels: usize,
    /// Items the last ink pass drew (tests).
    #[cfg(test)]
    cmyk_pass_items: usize,
    pub threads: u16,
    pub stats: FrameStats,
    slice_seen: bool,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
