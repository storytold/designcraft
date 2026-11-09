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
//!   losslessly. Compression ([`PdfOptions::images`]) bicubic-downsamples colour, greyscale and
//!   monochrome images above a resolution (pixel width over the narrowest placement) and can
//!   re-encode opaque colour and grey ones as JPEG ([`PdfOptions::compress_images`] at quality 90
//!   where it gives none); a JPEG is re-encoded only when downsampled or converted (below). CMYK
//!   rasters keep their inks (TIFF, Photoshop, JPEG) and, with an Image Quality, go in as CMYK
//!   JPEG; PDF/A takes them as RGB. PDF/X-4 and PDF/A images are written without interpolation.
//!   In PDF/X-4 an RGB image without a profile of its own carries sRGB
//!   (`assets/icc/sRGB-v2-magic.icc`).
//! - **Output colour conversion** ([`PdfOptions::output`]): Convert to Destination turns RGB
//!   colours and rasters into a CMYK destination profile, and working-space CMYK too when the
//!   destination is another profile; Preserve Numbers keeps CMYK numbers. The destination is the
//!   blending space and, in PDF/X, the output intent. Grey stays grey, placed PDF and SVG content
//!   is not converted, and PDF/A is left RGB with a warning.
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

mod color_out;
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
    BookletKind, BookletOptions, booklet_pairs, drawn_children, export_booklet, export_pdf, export_pdf_with_report, item_exported, layer_exported,
    merge_pdfs, print_color, sheet_spreads, shown_parent_items,
};
pub use pdfx::{check_pdfx4, has_rgb_groups, make_pdfx4, pdfx_version};
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

/// Export PDF › Compression for one class of raster images (colour, greyscale or monochrome).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageClass {
    /// Bicubic-downsample to this resolution (ppi) the images whose effective resolution (pixel
    /// width over the narrowest placed width) is above `above`; `None` keeps every pixel.
    pub downsample_to: Option<f64>,
    /// The resolution (ppi) above which `downsample_to` applies; one below `downsample_to` counts
    /// as `downsample_to` (an image is never upsampled).
    pub above: f64,
    /// JPEG quality 1–100; `None` = lossless (Flate). Images with transparency and monochrome
    /// images are always lossless.
    pub quality: Option<u8>,
}

impl ImageClass {
    /// Every pixel, lossless.
    pub const LOSSLESS: ImageClass = ImageClass { downsample_to: None, above: 0.0, quality: None };

    /// The JPEG quality of an Image Quality menu entry (`maximum`, `high`, `medium`, `low`,
    /// `minimum`, in any case).
    pub fn quality_named(name: &str) -> Option<u8> {
        match name.trim().to_ascii_lowercase().as_str() {
            "maximum" => Some(100),
            "high" => Some(80),
            "medium" => Some(60),
            "low" => Some(40),
            "minimum" => Some(20),
            _ => None,
        }
    }

    /// Whether this class downsamples any image.
    pub fn downsamples(&self) -> bool {
        self.downsample_to.is_some_and(|t| t.is_finite() && t > 0.0)
    }
}

/// Export PDF › Compression: downsampling and compression per image class.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageCompression {
    pub color: ImageClass,
    pub gray: ImageClass,
    /// Monochrome (black-and-white) images: `quality` is ignored, they are always lossless.
    pub mono: ImageClass,
}

impl ImageCompression {
    /// Every pixel of every image, lossless (`PdfOptions::compress_images` may still add JPEG).
    pub const NONE: ImageCompression = ImageCompression { color: ImageClass::LOSSLESS, gray: ImageClass::LOSSLESS, mono: ImageClass::LOSSLESS };

    /// Whether any class downsamples (and so needs the images' placed widths).
    pub fn downsamples(&self) -> bool {
        [self.color, self.gray, self.mono].iter().any(ImageClass::downsamples)
    }
}

impl Default for ImageCompression {
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
    /// Convert to Destination: RGB becomes CMYK through the destination profile, and CMYK in the
    /// document's working CMYK space is converted to the destination when that is another profile.
    ToDestination,
    /// Convert to Destination (Preserve Numbers): RGB becomes CMYK; CMYK keeps its numbers.
    PreserveNumbers,
}

impl ColorConversion {
    /// From a command parameter: `none`, `destination`, `preserveNumbers` (or their dialog names).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().replace(['-', '_', ' ', '(', ')'], "").as_str() {
            "none" | "nocolorconversion" => Some(Self::None),
            "destination" | "converttodestination" => Some(Self::ToDestination),
            "preservenumbers" | "converttodestinationpreservenumbers" => Some(Self::PreserveNumbers),
            _ => None,
        }
    }
}

/// Export PDF › Output.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Output {
    pub conversion: ColorConversion,
    /// The destination CMYK profile, by the name the colour settings know it by (a built-in or
    /// loaded profile); `None` = the working CMYK space. Converting PDFs carry it as their output
    /// intent (PDF/X) and blend transparency in it.
    pub destination: Option<String>,
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
    pub marks: Marks,
    pub standard: Standard,
    /// Re-encode raster images whose class has no JPEG quality of its own (`images`) as JPEG
    /// quality 90 when they have no transparency. Off = lossless. A JPEG is passed through
    /// unchanged unless it is downsampled or converted; a converted (CMYK) raster is never made
    /// JPEG by this alone.
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
    /// Creation date as Unix seconds (UTC). `None` takes `SOURCE_DATE_EPOCH` when it names a date
    /// a PDF can hold, else the clock (native) / omitted (wasm).
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
    // The pages selected so far, for ranges that overlap (`1-9, 2-8, …`): a set, not a search.
    let mut selected = vec![false; count];
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
            if let Some(new) = selected.get_mut(p - 1).filter(|s| !**s) {
                *new = true;
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
