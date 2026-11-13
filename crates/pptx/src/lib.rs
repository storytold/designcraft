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

fn set_picture_media(
    shapes: &mut [craft_pptx::Shape],
    names: &[(u32, String, Option<String>)],
    ids: &std::collections::HashMap<String, craft_pptx::MediaId>,
) -> Result<()> {
    for shape in shapes {
        if let craft_pptx::ShapeKind::Picture { fill } = &mut shape.kind {
            let (name, svg) = names
                .iter()
                .find(|(id, _, _)| *id == shape.id.0)
                .map(|(_, name, svg)| (name, svg))
                .ok_or_else(|| PptxError::Write(format!("picture {} has no media mapping", shape.id.0)))?;
            fill.media = *ids.get(name).ok_or_else(|| PptxError::Write(format!("picture media `{name}` is missing")))?;
            fill.svg = svg.as_ref().and_then(|name| ids.get(name).copied());
        }
        if let craft_pptx::ShapeKind::Group { children, .. } = &mut shape.kind {
            set_picture_media(children, names, ids)?;
        }
    }
    Ok(())
}

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
        return Err(PptxError::BadPage(bad.saturating_add(1)));
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
    let mut presentation = craft_pptx::Presentation { slide_size: craft_pptx::Size::new(size.0, size.1), ..Default::default() };
    presentation.props.title = title;
    presentation.props.author = opts.author.clone().unwrap_or_default();
    presentation.props.created = opts
        .created
        .and_then(|created| chrono::DateTime::<chrono::Utc>::from_timestamp(created, 0))
        .map(|created| created.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default();
    for (index, source) in slides.iter().enumerate() {
        presentation.slides.push(std::sync::Arc::new(craft_pptx::Slide {
            id: craft_pptx::SlideId(u32::try_from(index + 1).unwrap_or(u32::MAX)),
            name: source.name.clone(),
            shapes: source.shapes.clone(),
            ..Default::default()
        }));
    }
    let mut max_id = u32::try_from(slides.len()).unwrap_or(u32::MAX);
    for slide in &presentation.slides {
        craft_pptx::walk(&slide.shapes, &mut |shape, _| max_id = max_id.max(shape.id.0));
    }
    presentation.next_id = max_id;
    let mut media_ids = std::collections::HashMap::new();
    for media in &ex.media {
        let content_type = designcraft_images::mime(&media.bytes);
        let id = presentation
            .add_media(&media.name, content_type, media.bytes.clone())
            .ok_or_else(|| PptxError::Write("the presentation has no more media ids".into()))?;
        media_ids.insert(media.name.clone(), id);
    }
    for (source, slide) in slides.iter().zip(&mut presentation.slides) {
        let slide = std::sync::Arc::make_mut(slide);
        set_picture_media(&mut slide.shapes, &source.media, &media_ids)?;
    }
    for font in fonts {
        let mut embedded = craft_pptx::EmbeddedFont { typeface: font.family, ..Default::default() };
        let slots = [&mut embedded.regular, &mut embedded.bold, &mut embedded.italic, &mut embedded.bold_italic];
        for (slot, data) in slots.into_iter().zip(font.faces) {
            if let Some(data) = data {
                let name = format!("font{}.fntdata", presentation.embedded_fonts.len() + 1);
                *slot = presentation.add_media(&name, "application/x-fontdata", data);
                if slot.is_none() {
                    return Err(PptxError::Write("the presentation has no more media ids".into()));
                }
            }
        }
        presentation.embedded_fonts.push(embedded);
    }
    let bytes = craft_pptx::export(&presentation, &craft_pptx::Plain).map_err(|e| PptxError::Write(e.to_string()))?;
    Ok(ExportReport { bytes, slides: slides.len(), warnings: ex.warnings })
}
