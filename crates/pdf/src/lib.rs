//! DesignCraft PDF export (print), written with `krilla`.
//!
//! [`export_pdf`] writes one PDF page per document page (or per spread) and mirrors the renderer's
//! walk (`designcraft-render`): layers back to front, parent items first (with page-number markers
//! resolved for each page), then the spread's own items. What goes into the file:
//!
//! - **Boxes:** MediaBox = trim + bleed (+ room for printer's marks), TrimBox = the page, BleedBox =
//!   page + document bleed. Content is clipped to the bleed box.
//! - **Vectors:** fills (solid and gradient swatches), strokes with alignment, dashes, caps, joins
//!   and miter limits, corner options, opacity and blend modes (transparency groups), the simple
//!   drop shadow.
//! - **Colour:** CMYK as DeviceCMYK, RGB as DeviceRGB, Gray as DeviceGray, spot swatches (and their
//!   tints) as `/Separation` with the swatch's own values as the alternate space, `[Registration]`
//!   as `/Separation /All`.
//!   Gradient stops on swatches shade in their inks: one spot with tints of itself or `[Paper]`
//!   as a `/Separation` shading, a spot with process inks or a second spot as `/DeviceN`
//!   (process colorants first; not in PDF/A, which goes to process color with a warning), and in
//!   print a process gradient to `[Paper]` as DeviceCMYK. Plain-color stops keep their own
//!   space; gradients mixing RGB with CMYK or Gray are converted to RGB with an export warning.
//! - **Overprint:** Overprint Fill, Stroke and Gap, overprinting text and 100% [Black] (when
//!   Appearance of Black says so) as `/op` and `/OP` with `/OPM 1`; left out of PDF/A.
//! - **Images:** clipped to their frame; JPEG data is passed through, PNG/GIF/WebP are embedded
//!   losslessly (or re-encoded as JPEG with [`PdfOptions::compress_images`]). PDF/X-4 and PDF/A
//!   images are written without interpolation. In PDF/X-4 an RGB image without a profile of its own
//!   carries sRGB (`assets/icc/sRGB-v2-magic.icc`).
//! - **Text as real text:** the composed glyph runs are emitted with embedded, subsetted fonts and a
//!   Unicode mapping taken from the story text (ligatures, page numbers and inserted hyphens get the
//!   right characters), so text is selectable and searchable.
//! - **Marks:** crop marks, bleed marks and a page-information line, drawn in `[Registration]`.
//! - **Standards:** PDF/A-2b through krilla's validator and PDF/X-4 output intent/identification
//!   with built-in conformance checks. PDF/X-1a:2003 is the PDF/X-4 print file flattened by
//!   craft-prepress (no transparency, only DeviceCMYK, DeviceGray, Separation and DeviceN) and
//!   written by krilla as PDF 1.4 with the PDF/X-1a identification. `designcraft-cli validate-pdf` exposes those checks for CI.
//!
//! Not yet: tagged PDF, bookmarks, hyperlinks, layers as optional content.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod export;
mod gradient_inks;
pub mod inks;
mod links;
mod marks;
mod text;

mod forms;
mod optional_content;
mod pdfx;
mod pdfx1a;
mod transitions;
pub use export::{
    BookletKind, BookletOptions, booklet_pairs, export_booklet, export_pdf, export_pdf_with_report, merge_pdfs, print_color, sheet_spreads,
};
pub use pdfx::{check_pdfx4, has_rgb_groups, make_pdfx4};
pub use pdfx1a::check_pdfx1a;
pub use transitions::{add_blend_space, add_catalog_entries, add_page_entries, add_transitions};

/// PDF standard to target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Standard {
    /// Plain PDF 1.7.
    #[default]
    None,
    /// PDF/X-4 (PDF 1.6, trim/bleed boxes, output intent and identification).
    PdfX4,
    /// PDF/X-1a:2003 (PDF 1.4, CMYK and spot only, no transparency): the PDF/X-4 file flattened.
    PdfX1a,
    /// PDF/A-2b (archival; validated by krilla).
    PdfA2b,
}

impl Standard {
    /// `none`, `x4` / `pdfx4` / `PDF/X-4`, `x1a` / `pdfx1a` / `PDF/X-1a:2003`, `a2b` / `pdfa2b` /
    /// `PDF/A-2b`.
    pub fn parse(s: &str) -> Option<Self> {
        let k: String = s.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase();
        match k.as_str() {
            "" | "none" => Some(Self::None),
            "x4" | "pdfx4" | "pdfx42008" | "pdfx42010" => Some(Self::PdfX4),
            "x1a" | "pdfx1a" | "pdfx1a2003" => Some(Self::PdfX1a),
            "a2b" | "pdfa2b" => Some(Self::PdfA2b),
            _ => None,
        }
    }

    /// PDF/X: a CMYK output intent, trim boxes, no interpolation.
    pub fn is_pdfx(self) -> bool {
        matches!(self, Self::PdfX4 | Self::PdfX1a)
    }
}

/// Printer's marks (Export PDF › Marks and Bleeds).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marks {
    pub crop: bool,
    pub bleed: bool,
    /// Registration targets centred outside each edge of the trim.
    pub registration: bool,
    /// A row of process-ink and black-tint patches above the page.
    pub color_bars: bool,
    /// Document title, page name, date — in the slug area under the page.
    pub page_info: bool,
    /// Line weight in points (0.25 default).
    pub weight: f64,
    /// Distance of the marks from the trim edge in points (6 default). Marks never start inside
    /// the bleed.
    pub offset: f64,
}

impl Marks {
    pub const NONE: Marks = Marks { crop: false, bleed: false, registration: false, color_bars: false, page_info: false, weight: 0.25, offset: 6.0 };
    pub const ALL: Marks = Marks { crop: true, bleed: true, registration: true, color_bars: true, page_info: true, weight: 0.25, offset: 6.0 };
    pub fn any(&self) -> bool {
        self.crop || self.bleed || self.registration || self.color_bars || self.page_info
    }
}

impl Default for Marks {
    fn default() -> Self {
        Self::NONE
    }
}

/// How an image class is resampled when it is above its threshold resolution (Export PDF ›
/// Compression).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Downsample {
    /// Keep every pixel.
    #[default]
    None,
    /// Average downsampling: each new pixel is the area average of the pixels it covers.
    Average,
    /// Subsampling: each new pixel takes the source pixel at its centre (fastest, harshest).
    Subsample,
    /// Bicubic downsampling: a weighted cubic filter (smoothest tones).
    Bicubic,
}

impl Downsample {
    /// `none`, `average`, `subsample`, `bicubic`.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "none" | "" => Self::None,
            "average" => Self::Average,
            "subsample" | "subsampling" => Self::Subsample,
            "bicubic" => Self::Bicubic,
            _ => return None,
        })
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Average => "average",
            Self::Subsample => "subsample",
            Self::Bicubic => "bicubic",
        }
    }
}

/// How an image class is encoded (Export PDF › Compression).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageCompression {
    /// JPEG for continuous-tone images; ZIP for images with transparency or few colours, which
    /// JPEG would blur. JPEG files that need no change are passed through.
    #[default]
    Automatic,
    /// Lossy JPEG at the class's quality (images with transparency stay lossless).
    Jpeg,
    /// Lossless (Flate).
    Zip,
}

impl ImageCompression {
    /// `auto` / `automatic`, `jpeg`, `zip`.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "auto" | "automatic" => Self::Automatic,
            "jpeg" | "jpg" => Self::Jpeg,
            "zip" | "flate" | "lossless" => Self::Zip,
            _ => return None,
        })
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::Automatic => "auto",
            Self::Jpeg => "jpeg",
            Self::Zip => "zip",
        }
    }
}

/// JPEG image quality (Export PDF › Compression › Image Quality).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JpegQuality {
    Minimum,
    Low,
    Medium,
    High,
    #[default]
    Maximum,
}

impl JpegQuality {
    pub const ALL: [JpegQuality; 5] = [Self::Minimum, Self::Low, Self::Medium, Self::High, Self::Maximum];
    /// `minimum`, `low`, `medium`, `high`, `maximum`.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|q| q.id().eq_ignore_ascii_case(s))
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::Minimum => "minimum",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Maximum => "maximum",
        }
    }
    /// The JPEG encoder's quality (1–100).
    pub fn level(self) -> u8 {
        match self {
            Self::Minimum => 20,
            Self::Low => 40,
            Self::Medium => 60,
            Self::High => 80,
            Self::Maximum => 92,
        }
    }
}

/// Resampling and encoding for one class of images.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageSampling {
    pub downsample: Downsample,
    /// The resolution (pixels per inch at the placed size) to downsample to.
    pub ppi: f64,
    /// Only images above this resolution are downsampled (at least `ppi`).
    pub above_ppi: f64,
    pub compression: ImageCompression,
    pub quality: JpegQuality,
}

impl ImageSampling {
    /// Accepted resolutions, in pixels per inch.
    pub const PPI_RANGE: std::ops::RangeInclusive<f64> = 9.0..=2400.0;

    /// No resampling, encoded as `compression`.
    pub const fn keep(compression: ImageCompression) -> Self {
        Self { downsample: Downsample::None, ppi: 300.0, above_ppi: 450.0, compression, quality: JpegQuality::Maximum }
    }

    /// Downsampled to `ppi` when above `above_ppi`.
    pub const fn resample(downsample: Downsample, ppi: f64, above_ppi: f64, compression: ImageCompression, quality: JpegQuality) -> Self {
        Self { downsample, ppi, above_ppi, compression, quality }
    }
}

/// Export PDF › Compression: how placed raster images are resampled and encoded, by class.
/// Placed PDFs and SVGs are vectors and are left alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageOptions {
    /// RGB and CMYK images.
    pub color: ImageSampling,
    /// Grayscale images.
    pub gray: ImageSampling,
    /// Black-and-white images (only fully black or white pixels); always lossless.
    pub mono: ImageSampling,
    /// Leave out the image data outside each graphic's frame.
    pub crop_to_frames: bool,
}

impl Default for ImageOptions {
    /// Lossless and full resolution, as an export without these options writes images.
    fn default() -> Self {
        let keep = ImageSampling::keep(ImageCompression::Zip);
        Self { color: keep, gray: keep, mono: keep, crop_to_frames: false }
    }
}

/// Export PDF › Output › Color Conversion.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorConversion {
    /// Colours are written in the space they were defined in.
    #[default]
    None,
    /// Every process colour and image is converted to the destination profile.
    ToDestination,
    /// Colours already in the destination's model keep their numbers; the others are converted.
    PreserveNumbers,
}

impl ColorConversion {
    /// `none`, `destination`, `preserveNumbers`.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().replace(['_', '-', ' '], "").as_str() {
            "none" | "" => Self::None,
            "destination" | "todestination" | "converttodestination" => Self::ToDestination,
            "preservenumbers" | "destinationpreservenumbers" => Self::PreserveNumbers,
            _ => return None,
        })
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ToDestination => "destination",
            Self::PreserveNumbers => "preserveNumbers",
        }
    }
}

/// Export PDF › Output: colour conversion to a destination profile.
#[derive(Clone, Debug, PartialEq)]
pub struct OutputColor {
    pub conversion: ColorConversion,
    /// A profile name from `designcraft_color::cms::profiles()`; `None` = the document's
    /// working space of the destination model (CMYK for print output).
    pub destination: Option<String>,
    /// Write the destination profile as the file's output intent (always for PDF/X).
    pub include_profile: bool,
    /// Rendering intent of the conversion; `None` = Edit › Color Settings' intent.
    pub intent: Option<designcraft_color::cms::Intent>,
}

impl Default for OutputColor {
    fn default() -> Self {
        Self { conversion: ColorConversion::None, destination: None, include_profile: false, intent: None }
    }
}

/// Export options.
#[derive(Clone, Debug)]
pub struct PdfOptions {
    /// 0-based absolute page indices to export, in order; `None` = all pages.
    pub pages: Option<Vec<usize>>,
    /// One PDF page per spread (spreads containing any chosen page) instead of per page.
    pub spreads: bool,
    /// Include the document bleed (`DocSettings::bleed`) around each page.
    pub bleed: bool,
    /// Bleed for this export instead of the document's (top, bottom, inside, outside, in
    /// points), when `bleed` is on.
    pub bleed_values: Option<[f64; 4]>,
    /// Include the slug area (`DocSettings::slug`): its items print and the sheet grows to it.
    pub slug: bool,
    pub marks: Marks,
    pub standard: Standard,
    /// Re-encode non-JPEG raster images as JPEG (quality 90) when they have no transparency.
    /// Off = lossless (JPEG data is always passed through unchanged).
    pub compress_images: bool,
    /// Image resampling and encoding by class. `None` = the `compress_images` behaviour: every
    /// image at full resolution.
    pub images: Option<ImageOptions>,
    /// Colour conversion to a destination profile.
    pub output: OutputColor,
    /// Flate-compress content streams.
    pub compress: bool,
    /// Title for the metadata; `None` = the document title.
    pub title: Option<String>,
    pub author: Option<String>,
    /// Creation date as Unix seconds (UTC); `None` = now (native) / omitted (wasm).
    pub created: Option<i64>,
    /// Tagged PDF: stories become paragraphs in reading order, graphics figures with their alt
    /// text, parent-page items and printer's marks artifacts.
    pub tagged: bool,
    /// Interactive PDF: placed video and sound are embedded and play in Screen annotations.
    pub media: bool,
    /// PDF/X-1a: the resolution (pixels per inch) craft-prepress's flattener rasterizes complex
    /// transparent regions at.
    pub flatten_dpi: f32,
    /// Soft drop shadows drawn as their shadow color through a coverage mask, by item id (spread
    /// items only). Items without one draw the simple offset silhouette.
    pub shadows: std::sync::Arc<std::collections::HashMap<designcraft_doc::ItemId, ShadowMask>>,
}

/// A soft drop shadow's coverage over `rect` (spread coordinates): `width` × `height` greys,
/// 255 = the shadow color at full strength. Its opacity is in the coverage.
#[derive(Clone, Debug, PartialEq)]
pub struct ShadowMask {
    pub rect: designcraft_geom::Rect,
    pub width: u32,
    pub height: u32,
    pub coverage: std::sync::Arc<Vec<u8>>,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            pages: None,
            spreads: false,
            bleed: false,
            bleed_values: None,
            slug: false,
            marks: Marks::NONE,
            standard: Standard::None,
            compress_images: false,
            images: None,
            output: OutputColor::default(),
            compress: true,
            title: None,
            author: None,
            created: None,
            tagged: false,
            media: false,
            flatten_dpi: 300.0,
            shadows: Default::default(),
        }
    }
}

/// Bytes plus non-fatal warnings (features approximated or dropped).
#[derive(Clone, Debug)]
pub struct ExportReport {
    pub bytes: Vec<u8>,
    pub pages: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum PdfError {
    #[error("nothing to export (no pages selected)")]
    NoPages,
    #[error("page {0} does not exist")]
    BadPage(usize),
    #[error("bad page range `{0}`")]
    BadRange(String),
    #[error("PDF writer error: {0}")]
    Write(String),
}

pub type Result<T> = std::result::Result<T, PdfError>;

/// Parse a 1-based page range like `"1-3, 5, 8-"` into 0-based indices (`count` pages in the
/// document). `""` / `"all"` = every page.
pub fn parse_page_range(s: &str, count: usize) -> Result<Vec<usize>> {
    let t = s.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("all") {
        return Ok((0..count).collect());
    }
    let bad = || PdfError::BadRange(s.to_string());
    let num = |v: &str| v.trim().parse::<usize>().map_err(|_| bad());
    let mut out = Vec::new();
    for part in t.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => {
                let a = if a.trim().is_empty() { 1 } else { num(a)? };
                let b = if b.trim().is_empty() { count } else { num(b)? };
                (a, b)
            }
            None => {
                let v = num(part)?;
                (v, v)
            }
        };
        if a == 0 || b == 0 || a > b {
            return Err(bad());
        }
        for p in a..=b {
            if p > count {
                return Err(PdfError::BadPage(p));
            }
            if !out.contains(&(p - 1)) {
                out.push(p - 1);
            }
        }
    }
    Ok(out)
}

impl From<craft_pdf::write::WriteError> for PdfError {
    fn from(error: craft_pdf::write::WriteError) -> Self {
        Self::Write(error.to_string())
    }
}

#[cfg(test)]
mod output_tests;
#[cfg(test)]
mod print_tests;
#[cfg(test)]
mod rtl_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod x1a_tests;

// Run the same public API/output contracts with the unit-only colliding cache hasher as well
// as the production RandomState used by the integration-test executable.
#[cfg(test)]
extern crate self as designcraft_pdf;
#[cfg(test)]
#[path = "../tests/support/image_content_cache_contract.rs"]
mod image_content_cache_collision_exports;
#[cfg(test)]
#[path = "../tests/support/image_fixtures.rs"]
mod image_fixtures;
