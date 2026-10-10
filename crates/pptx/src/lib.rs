//! PowerPoint export (File › Export › PowerPoint).
//!
//! Every exported page becomes a slide of native, editable objects: frames become shapes (preset
//! rectangles and ellipses, custom geometry otherwise) with their fills, strokes and effects,
//! placed images become pictures cropped like their frames, and text frames become text boxes.
//!
//! Text keeps DesignCraft's composition by default ([`LineBreaks::Keep`]): each composed line ends
//! in a line break, justified word spacing and tracking become character spacing, and leading
//! becomes percentage line spacing (PowerPoint rounds exact point spacing to whole points; its
//! 100% line is 1.2 em for every font, so a percentage is exact). A threaded story is split at its
//! frames, and the lines a text wrap pushes aside become boxes of their own, since PowerPoint has
//! neither threads nor wraps. [`LineBreaks::Reflow`] instead writes one box per frame with native
//! columns and lets PowerPoint break the lines.
//!
//! Parent-page items are copied onto each slide; layers are flattened in stacking order. What has
//! no PowerPoint equivalent (blend modes, type on a path, some effects) is approximated or
//! dropped and reported in [`ExportReport::warnings`]; [`needs_raster`] names the objects a caller
//! should rasterise first.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod fonts;
mod package;
mod slide;
mod table;
mod text;
mod xml;

#[cfg(test)]
mod tests;

use designcraft_compose::Cache;
use designcraft_doc::Document;

pub use slide::needs_raster;

/// How text frames become text boxes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineBreaks {
    /// Keep DesignCraft's line breaks, word spacing and leading (looks the same; edits don't
    /// re-break lines).
    #[default]
    Keep,
    /// One box per frame with native columns; PowerPoint breaks the lines (edits reflow; lines,
    /// hyphenation and wraps differ).
    Reflow,
}

#[derive(Clone, Debug, Default)]
pub struct PptxOptions {
    /// Document pages to export (0-based positions); all when `None`.
    pub pages: Option<Vec<usize>>,
    pub line_breaks: LineBreaks,
    pub title: Option<String>,
    pub author: Option<String>,
    /// Creation time (Unix seconds) for the document properties; none when `None`.
    pub created: Option<i64>,
    /// Embed the TrueType fonts the text uses (when their licences allow it), so the slides
    /// look the same where they aren't installed.
    pub embed_fonts: bool,
}

/// The exported presentation and what could not be represented exactly.
#[derive(Clone, Debug, Default)]
pub struct ExportReport {
    pub bytes: Vec<u8>,
    pub slides: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum PptxError {
    #[error("the document has no pages to export")]
    NoPages,
    #[error("page {0} does not exist")]
    BadPage(usize),
    #[error("could not write the presentation: {0}")]
    Write(String),
}

pub type Result<T> = std::result::Result<T, PptxError>;

/// Export `doc` as a PowerPoint presentation.
pub fn export_pptx(doc: &Document, cache: &Cache, opts: &PptxOptions) -> Result<ExportReport> {
    let count = doc.page_count();
    let pages: Vec<usize> = match &opts.pages {
        Some(v) => v.clone(),
        None => (0..count).collect(),
    };
    if pages.is_empty() {
        return Err(PptxError::NoPages);
    }
    if let Some(bad) = pages.iter().find(|p| **p >= count) {
        return Err(PptxError::BadPage(bad + 1));
    }
    let mut ex = slide::Exporter::new(doc, cache, opts);
    let mut slides = Vec::with_capacity(pages.len());
    for abs in &pages {
        slides.push(ex.slide(*abs)?);
    }
    let size = slides.first().map(|s| s.size).ok_or(PptxError::NoPages)?;
    if slides.iter().any(|s| (s.size.0 - size.0).abs() > 0.5 || (s.size.1 - size.1).abs() > 0.5) {
        ex.warn("pages of different sizes: PowerPoint has one slide size, so every slide uses the first page's");
    }
    let fonts = if opts.embed_fonts {
        let (fonts, skipped) = ex.fonts.embedded();
        if !skipped.is_empty() {
            ex.warn(format!("fonts not embedded: {}", skipped.join(", ")));
        }
        fonts
    } else {
        let mut names: Vec<String> = ex.fonts.used.iter().map(|(f, _)| f.family.clone()).collect();
        names.dedup();
        if !names.is_empty() {
            ex.warn(format!("fonts are not embedded; the slides need {} installed to look the same", names.join(", ")));
        }
        Vec::new()
    };
    let title = opts.title.clone().unwrap_or_else(|| doc.title.clone());
    let bytes = package::write(&package::Package {
        size,
        slides: &slides,
        media: &ex.media,
        fonts: &fonts,
        title: &title,
        author: opts.author.as_deref().unwrap_or(""),
        created: opts.created,
    })?;
    Ok(ExportReport { bytes, slides: slides.len(), warnings: ex.warnings })
}
