//! Document → PDF. The item walk mirrors `designcraft-render`'s `draw_spread` / `draw_item`.

#[cfg(test)]
#[path = "registration_tests.rs"]
pub(crate) mod registration_tests;

use std::collections::HashMap;
use std::sync::Arc;

use craft_pdf::write::backend as krilla;
use craft_pdf::write::{self, Metadata, Writer};
use designcraft_color::swatch::{ColorType, SwatchValue};
use designcraft_color::{BlendMode as DcBlend, Color, GradientKind};
use designcraft_compose::Cache;
use designcraft_doc::{AssetId, Content, Document, Item, PageSide, Spread, SpreadRef, StrokeAlign, StrokeType};
use designcraft_geom::kurbo::{PathEl, Shape as _};
use designcraft_geom::{Affine, BezPath, Rect, Vec2, corners};
use krilla::color::{cmyk, luma, rgb};
use krilla::configure::{Archival, PdfVersion};
use krilla::geom::{Path, PathBuilder, Size, Transform};
use krilla::image::Image;
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{Fill, FillRule, LinearGradient, RadialGradient, SpreadMethod, Stop, Stroke, StrokeDash};
use krilla::surface::Surface;
use krilla::tagging::{Artifact, ArtifactType, ContentTag, Identifier, Tag, TagGroup, TagTree};

use crate::{ExportReport, PdfError, PdfOptions, Result, Standard};

mod booklet;
mod common;
mod graphics;
mod images;
mod metadata;
mod paint;
mod pipeline;
mod walk;

pub use booklet::{BookletKind, BookletOptions, booklet_pairs, export_booklet, merge_pdfs};
use common::TagEntry;
pub(crate) use common::{norm, q, registration, regular, solid_fill, tf, to_path};
use graphics::CoverageImage;
#[cfg(test)]
use images::ConstantImageHasher;
pub(crate) use images::SRGB_ICC;
use images::{CmykImage, GreyImage, ImageContentCache, RasterMode, SrgbImage, embedded_icc, is_grey, is_grey_icc};
pub(crate) use metadata::civil;
pub(crate) use pipeline::{MARK_LEN, Sheet, exported_items, mark_start};
pub use pipeline::{export_pdf, export_pdf_with_report, print_color, sheet_spreads};
#[cfg(test)]
impl std::hash::Hasher for ConstantImageHasher {
    fn finish(&self) -> u64 {
        0
    }
    fn write(&mut self, _: &[u8]) {}
}

pub(crate) struct Exporter<'a> {
    pub doc: &'a Document,
    /// The document's inks with its Ink Manager settings.
    pub(crate) inks: craft_prepress::InkSet,
    pub cache: &'a Cache,
    /// Each story's composition per page it is shown on, for this export: the document doesn't
    /// change while it runs, and asking the cache checks every frame of the story again.
    composed: HashMap<(designcraft_doc::StoryId, Option<usize>), Arc<designcraft_compose::ComposedStory>>,
    pub opts: &'a PdfOptions,
    /// Current sheet's visible area (spread space) for culling.
    pub clip: Rect,
    pub warnings: Vec<String>,
    drawing_items: Vec<designcraft_doc::ItemId>,
    images: HashMap<AssetId, Option<Image>>,
    // Immutable export options (including rgb_only) make bytes the complete conversion key.
    // PDF-page/SVG/media routing precedes this path. Failures remain per AssetId.
    images_by_content: ImageContentCache,
    #[cfg(test)]
    image_conversions: usize,
    /// Placed PDFs (embedded as vector pages) and their page counts.
    pdfs: HashMap<(AssetId, Vec<hayro_syntax::object::ObjectIdentifier>), Option<(krilla::pdf::PdfDocument, usize)>>,
    variants: HashMap<(AssetId, Option<Vec<String>>), crate::optional_content::Variant>,
    has_optional_content: bool,
    #[cfg(test)]
    pdf_preparations: usize,
    layer_error: Option<PdfError>,
    /// The PDF version written; placed PDFs must not be newer.
    version: PdfVersion,
    /// Placed PDFs too new to embed in this version ("name is PDF 2.0"): the export fails.
    too_new: Vec<String>,
    /// Parsed placed SVGs.
    svgs: HashMap<AssetId, Option<Arc<designcraft_images::usvg::Tree>>>,
    pub fonts: HashMap<u32, Option<krilla::text::Font>>,
    pub reverse_cmaps: HashMap<u32, Arc<HashMap<u32, char>>>,
    /// Convert CMYK to RGB (PDF/A: krilla needs a CMYK output profile we don't ship yet).
    pub rgb_only: bool,
    /// Tagged PDF: the structure in reading order (stories gather their frames' content).
    tags: Vec<TagEntry>,
    story_tags: HashMap<designcraft_doc::StoryId, Vec<Identifier>>,
    /// While drawing a story's frame for the structure tree: the story (text runs are tagged
    /// per paragraph, everything else in the frame is an artifact).
    pub(crate) tag_story: Option<designcraft_doc::StoryId>,
    /// While drawing a frame's text: the frame's box in text space, which a gradient on the text
    /// spans (as InDesign lays it across the frame).
    pub(crate) text_area: Option<Rect>,
    /// Marked content of each (story, paragraph index).
    pub(crate) para_tags: HashMap<(designcraft_doc::StoryId, usize), Vec<Identifier>>,
}

impl CoverageImage {
    fn new(rgba: &image::RgbaImage) -> Option<Self> {
        let size = rgba.dimensions();
        if size.0 == 0 || size.1 == 0 {
            return None;
        }
        let grey = rgba.pixels().map(|p| ((u16::from(255 - p[0]) * u16::from(p[3]) + 127) / 255) as u8).collect();
        Some(Self { grey: Arc::new(grey), size })
    }

    /// Coverage given directly, one byte per pixel; `None` when the size doesn't match.
    fn from_grey(grey: Arc<Vec<u8>>, size: (u32, u32)) -> Option<Self> {
        let n = usize::try_from(u64::from(size.0) * u64::from(size.1)).ok()?;
        (n > 0 && grey.len() == n).then_some(Self { grey, size })
    }
}

impl krilla::image::CustomImage for CoverageImage {
    fn color_channel(&self) -> &[u8] {
        &self.grey
    }
    fn alpha_channel(&self) -> Option<&[u8]> {
        None
    }
    fn bits_per_component(&self) -> krilla::image::BitsPerComponent {
        krilla::image::BitsPerComponent::Eight
    }
    fn size(&self) -> (u32, u32) {
        self.size
    }
    fn icc_profile(&self) -> Option<&[u8]> {
        None
    }
    fn color_space(&self) -> krilla::image::ImageColorspace {
        krilla::image::ImageColorspace::Luma
    }
}

impl RasterMode {
    fn for_standard(standard: Standard) -> Self {
        Self { interpolate: standard == Standard::None, tag_rgb: standard.is_pdfx() }
    }
}

impl SrgbImage {
    fn new(rgba: image::RgbaImage) -> Option<Self> {
        let size = rgba.dimensions();
        let pixels = usize::try_from(size.0).ok()?.checked_mul(usize::try_from(size.1).ok()?)?;
        let mut rgb = Vec::with_capacity(pixels.checked_mul(3)?);
        let mut alpha = Vec::with_capacity(pixels);
        for p in rgba.pixels() {
            rgb.extend_from_slice(&p.0[..3]);
            alpha.push(p.0[3]);
        }
        let alpha = alpha.iter().any(|a| *a < 255).then(|| Arc::new(alpha));
        Some(Self { rgb: Arc::new(rgb), alpha, size })
    }
}

impl krilla::image::CustomImage for SrgbImage {
    fn color_channel(&self) -> &[u8] {
        &self.rgb
    }
    fn alpha_channel(&self) -> Option<&[u8]> {
        self.alpha.as_deref().map(Vec::as_slice)
    }
    fn bits_per_component(&self) -> krilla::image::BitsPerComponent {
        krilla::image::BitsPerComponent::Eight
    }
    fn size(&self) -> (u32, u32) {
        self.size
    }
    fn icc_profile(&self) -> Option<&[u8]> {
        Some(SRGB_ICC)
    }
    fn color_space(&self) -> krilla::image::ImageColorspace {
        krilla::image::ImageColorspace::Rgb
    }
}

impl GreyImage {
    /// `None` when `data` isn't grey or can't be decoded.
    fn new(data: &[u8]) -> Option<Self> {
        if !is_grey(data) {
            return None;
        }
        let img = image::load_from_memory(data).ok()?.to_luma_alpha8();
        let size = img.dimensions();
        let pixels = usize::try_from(size.0).ok()?.checked_mul(usize::try_from(size.1).ok()?)?;
        if pixels == 0 {
            return None;
        }
        let (mut grey, mut alpha) = (Vec::with_capacity(pixels), Vec::with_capacity(pixels));
        for p in img.pixels() {
            grey.push(p.0[0]);
            alpha.push(p.0[1]);
        }
        let alpha = alpha.iter().any(|a| *a < 255).then(|| Arc::new(alpha));
        let icc = embedded_icc(data).filter(|p| is_grey_icc(p)).map(Arc::new);
        Some(Self { grey: Arc::new(grey), alpha, size, icc })
    }
}

impl krilla::image::CustomImage for GreyImage {
    fn color_channel(&self) -> &[u8] {
        &self.grey
    }
    fn alpha_channel(&self) -> Option<&[u8]> {
        self.alpha.as_deref().map(Vec::as_slice)
    }
    fn bits_per_component(&self) -> krilla::image::BitsPerComponent {
        krilla::image::BitsPerComponent::Eight
    }
    fn size(&self) -> (u32, u32) {
        self.size
    }
    fn icc_profile(&self) -> Option<&[u8]> {
        self.icc.as_deref().map(Vec::as_slice)
    }
    fn color_space(&self) -> krilla::image::ImageColorspace {
        krilla::image::ImageColorspace::Luma
    }
}

impl krilla::image::CustomImage for CmykImage {
    fn color_channel(&self) -> &[u8] {
        &self.0.cmyk
    }
    fn alpha_channel(&self) -> Option<&[u8]> {
        self.0.alpha.as_deref()
    }
    fn bits_per_component(&self) -> krilla::image::BitsPerComponent {
        krilla::image::BitsPerComponent::Eight
    }
    fn size(&self) -> (u32, u32) {
        (self.0.width, self.0.height)
    }
    fn icc_profile(&self) -> Option<&[u8]> {
        None
    }
    fn color_space(&self) -> krilla::image::ImageColorspace {
        krilla::image::ImageColorspace::Cmyk
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod unwind_tests {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        static FAIL_NEXT_SURFACE: Cell<bool> = const { Cell::new(false) };
    }

    // Fail with live backend drawing state, so a missing guard or an unpatched destructor
    // makes the child fail. Thread-local state keeps unrelated export tests independent.
    #[allow(clippy::panic)] // Test-only fault injection exercises recovery with a live surface.
    pub(super) fn inject(surface: &mut krilla::surface::Surface<'_>) {
        if FAIL_NEXT_SURFACE.replace(false) {
            surface.push_transform(&krilla::geom::Transform::from_translate(2.0, 3.0));
            panic!("synthetic export drawing failure");
        }
    }

    #[test]
    fn export_entry_points_recover_in_a_subprocess() {
        const CHILD: &str = "DESIGNCRAFT_PDF_EXPORT_PANIC_PROBE";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "export::unwind_tests::export_entry_points_recover_in_a_subprocess", "--nocapture"])
                .env(CHILD, "1")
                .output()
                .unwrap();
            assert!(output.status.success(), "export child failed: {:?}\n{}", output.status, String::from_utf8_lossy(&output.stderr));
            assert!(String::from_utf8_lossy(&output.stdout).contains("all three export entry points recovered"));
            return;
        }
        let doc = Document::new(&designcraft_doc::build::NewDocument { pages: 1, ..Default::default() });
        let cache = Cache::new();
        let source = export_pdf(&doc, &cache, &Default::default()).unwrap();
        let parts = [(source, vec![(612.0, 792.0)])];
        for entry in ["document", "booklet", "merge"] {
            let export = || match entry {
                "document" => export_pdf(&doc, &cache, &Default::default()),
                "booklet" => export_booklet(&doc, &cache, &Default::default()).map(|report| report.bytes),
                _ => merge_pdfs(&parts, None),
            };
            FAIL_NEXT_SURFACE.set(true);
            assert!(matches!(export(), Err(PdfError::Write(message)) if message.contains("synthetic export drawing failure")), "{entry}");
            let fresh = export().unwrap();
            assert!(!hayro_syntax::Pdf::new(Arc::new(fresh)).unwrap().pages().is_empty(), "fresh {entry} export must still work");
        }
        println!("all three export entry points recovered");
    }
}
