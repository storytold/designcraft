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
//!   Gradients with mixed output colour spaces are converted to RGB with an export warning;
//!   homogeneous gradients keep their existing colour space (subject to PDF/A conversion).
//! - **Images:** clipped to their frame; JPEG data is passed through, PNG/GIF/WebP are embedded
//!   losslessly (or re-encoded as JPEG with [`PdfOptions::compress_images`]). PDF/X-4 and PDF/A
//!   images are written without interpolation. In PDF/X-4 an RGB image without a profile of its own
//!   carries sRGB (`assets/icc/sRGB-v2-magic.icc`).
//! - **Text as real text:** the composed glyph runs are emitted with embedded, subsetted fonts and a
//!   Unicode mapping taken from the story text (ligatures, page numbers and inserted hyphens get the
//!   right characters), so text is selectable and searchable.
//! - **Marks:** crop marks, bleed marks and a page-information line, drawn in `[Registration]`.
//! - **Standards:** PDF/A-2b through krilla's validator and PDF/X-4 output intent/identification
//!   with built-in conformance checks. `designcraft-cli validate-pdf` exposes those checks for CI.
//!
//! Not yet: tagged PDF, bookmarks, hyperlinks, overprint, layers as optional content.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod color_out;
mod export;
mod links;
mod marks;
mod text;

mod forms;
mod pdfx;
mod transitions;
pub use export::{BookletKind, BookletOptions, booklet_pairs, export_booklet, export_pdf, export_pdf_with_report, merge_pdfs, sheet_spreads};
pub use pdfx::{check_pdfx4, cmyk_group_spaces, has_rgb_groups, make_pdfx4};
pub use transitions::{add_blend_space, add_catalog_entries, add_page_entries, add_transitions};

/// PDF standard to target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Standard {
    /// Plain PDF 1.7.
    #[default]
    None,
    /// PDF/X-4 (PDF 1.6, trim/bleed boxes, output intent and identification).
    PdfX4,
    /// PDF/A-2b (archival; validated by krilla).
    PdfA2b,
}

impl Standard {
    /// `none`, `x4` / `pdfx4` / `PDF/X-4`, `a2b` / `pdfa2b` / `PDF/A-2b`.
    pub fn parse(s: &str) -> Option<Self> {
        let k: String = s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        match k.as_str() {
            "" | "none" => Some(Self::None),
            "x4" | "pdfx4" | "pdfx42008" | "pdfx42010" => Some(Self::PdfX4),
            "a2b" | "pdfa2b" => Some(Self::PdfA2b),
            _ => None,
        }
    }
}

/// Printer's marks (Export PDF › Marks and Bleeds).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marks {
    pub crop: bool,
    pub bleed: bool,
    /// Document title, page name, date — in the slug area under the page.
    pub page_info: bool,
    /// Line weight in points (0.25 default).
    pub weight: f64,
    /// Distance of the marks from the trim edge in points (6 default). Marks never start inside
    /// the bleed.
    pub offset: f64,
}

impl Marks {
    pub const NONE: Marks = Marks { crop: false, bleed: false, page_info: false, weight: 0.25, offset: 6.0 };
    pub const ALL: Marks = Marks { crop: true, bleed: true, page_info: true, weight: 0.25, offset: 6.0 };
    pub fn any(&self) -> bool {
        self.crop || self.bleed || self.page_info
    }
}

impl Default for Marks {
    fn default() -> Self {
        Self::NONE
    }
}

/// Export PDF › Output › Color Conversion.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorConversion {
    /// Colours and images stay in their own spaces.
    #[default]
    None,
    /// RGB colours and images become CMYK through the destination profile.
    ToDestination,
    /// As `ToDestination`; CMYK numbers are kept (they are in both modes today: placed CMYK is
    /// untagged).
    PreserveNumbers,
}

impl ColorConversion {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().replace(['-', '_', ' '], "").as_str() {
            "" | "none" | "nocolorconversion" => Some(Self::None),
            "destination" | "converttodestination" | "convert" => Some(Self::ToDestination),
            "preservenumbers" | "converttodestinationpreservenumbers" => Some(Self::PreserveNumbers),
            _ => None,
        }
    }
    pub fn id(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ToDestination => "destination",
            Self::PreserveNumbers => "preserveNumbers",
        }
    }
}

/// Export PDF › Output.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Output {
    pub conversion: ColorConversion,
    /// Destination CMYK profile (a name from the colour settings' profiles); `None` = the
    /// working CMYK.
    pub destination: Option<String>,
}

/// Export PDF › Compression, for one class of raster images (colour, greyscale or monochrome).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageClass {
    /// Bicubic-downsample to this resolution (ppi) the images whose effective resolution (pixels
    /// over the narrowest placement) is above `above`; `None` keeps every pixel.
    pub downsample_to: Option<f64>,
    /// The resolution (ppi) above which `downsample_to` applies.
    pub above: f64,
    /// JPEG quality 1–100; `None` = lossless (Flate). Images with transparency and monochrome
    /// images are always lossless.
    pub quality: Option<u8>,
}

impl ImageClass {
    /// Every pixel, lossless.
    pub const LOSSLESS: ImageClass = ImageClass { downsample_to: None, above: 0.0, quality: None };
    /// JPEG quality names as InDesign's Image Quality menu lists them.
    pub fn quality_named(name: &str) -> Option<u8> {
        match name {
            "maximum" => Some(100),
            "high" => Some(80),
            "medium" => Some(60),
            "low" => Some(40),
            "minimum" => Some(20),
            _ => None,
        }
    }
}

/// Export PDF › Compression: downsampling and compression per image class.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageCompression {
    pub color: ImageClass,
    pub gray: ImageClass,
    /// Monochrome (1-bit) images: `quality` is ignored, they are always lossless.
    pub mono: ImageClass,
}

impl ImageCompression {
    /// Every pixel of every image, lossless (`PdfOptions::compress_images` may still add JPEG).
    pub const NONE: ImageCompression = ImageCompression { color: ImageClass::LOSSLESS, gray: ImageClass::LOSSLESS, mono: ImageClass::LOSSLESS };
}

impl Default for ImageCompression {
    fn default() -> Self {
        Self::NONE
    }
}

/// Export options.
#[derive(Clone, Debug)]
pub struct PdfOptions {
    /// 0-based absolute page indices to export, in order; `None` = all pages.
    pub pages: Option<Vec<usize>>,
    /// One PDF page per spread (spreads containing any chosen page) instead of per page.
    pub spreads: bool,
    /// Include the bleed around each page: the document bleed (`DocSettings::bleed`), or
    /// `bleed_override`.
    pub bleed: bool,
    /// Bleed amounts (top, bottom, inside, outside) in points used instead of the document's
    /// (Marks and Bleeds › Use Document Bleed Settings off). Only with `bleed`.
    pub bleed_override: Option<[f64; 4]>,
    /// Include the slug area (`DocSettings::slug`) in the page's media box.
    pub slug: bool,
    pub marks: Marks,
    pub standard: Standard,
    /// Re-encode raster images with no JPEG quality of their own (`images`) as JPEG quality 90
    /// when they have no transparency. Off = lossless (JPEG data is passed through unchanged
    /// unless it is downsampled).
    pub compress_images: bool,
    /// Compression: downsampling and JPEG quality per image class.
    pub images: ImageCompression,
    /// Output: colour conversion to a CMYK destination.
    pub output: Output,
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
}

impl PdfOptions {
    /// The bleed around each page (top, bottom, inside, outside): the override, else the
    /// document's; none without `bleed`. Negative or non-finite values count as 0.
    pub fn bleed_amounts(&self, doc: &designcraft_doc::Document) -> [f64; 4] {
        if !self.bleed {
            return [0.0; 4];
        }
        self.bleed_override.unwrap_or(doc.settings.bleed).map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 })
    }

    /// The slug area (top, bottom, inside, outside) added to the media box; none without `slug`.
    pub fn slug_amounts(&self, doc: &designcraft_doc::Document) -> [f64; 4] {
        if !self.slug {
            return [0.0; 4];
        }
        doc.settings.slug.map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 })
    }
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            pages: None,
            spreads: false,
            bleed: false,
            bleed_override: None,
            slug: false,
            marks: Marks::NONE,
            standard: Standard::None,
            compress_images: false,
            images: ImageCompression::NONE,
            output: Output::default(),
            compress: true,
            title: None,
            author: None,
            created: None,
            tagged: false,
            media: false,
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
mod tests;

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
