//! DesignCraft's [`DocumentConverter`]: a print-ready PDF (the page export) becomes PWG raster, for a
//! printer that lists raster but not PDF.
//!
//! The registry decides *whether* to convert (the printer does not take the document's format) and
//! *at what resolution* (the job's own, else the printer's listed one nearest 300 dpi); this is the
//! *how*, with the renderer DesignCraft already has for placed PDFs (`designcraft-render`, hayro) and
//! craft-print's neutral [`pwg`] encoder. It makes nothing but PWG raster, and starts from nothing
//! but PDF; any other format the printer does not list is [`ErrorKind::UnsupportedFormat`] from the
//! registry.

use craft_print::{ConvertContext, Document, DocumentConverter, DocumentFormat, Duplex, ErrorKind, OwnedDocument, PrintError, pwg};

/// The most the renderer scales a page (`designcraft_render::render_pdf_page` caps at 8x).
const MAX_SCALE: f64 = 8.0;

fn failed(why: impl Into<String>) -> PrintError {
    PrintError::new(ErrorKind::Failed, why)
}

/// PDF to PWG raster. Stateless: register one with [`super::register_pdf_converter`].
#[derive(Clone, Copy, Debug, Default)]
pub struct PdfToRaster;

impl DocumentConverter for PdfToRaster {
    fn targets(&self, from: DocumentFormat) -> Vec<DocumentFormat> {
        if from == DocumentFormat::Pdf { vec![DocumentFormat::PwgRaster] } else { Vec::new() }
    }

    fn convert(&self, document: Document<'_>, target: DocumentFormat, context: &ConvertContext) -> Result<OwnedDocument, PrintError> {
        if document.format != DocumentFormat::Pdf || target != DocumentFormat::PwgRaster {
            return Err(PrintError::new(
                ErrorKind::UnsupportedFormat,
                format!("this converter makes PWG raster from PDF, not {} from {}", target.mime(), document.format.mime()),
            ));
        }
        let count = designcraft_render::pdf_page_count(document.bytes)
            .filter(|n| *n > 0)
            .ok_or_else(|| failed("the document has no pages that can be rendered"))?;
        let (dx, dy) = (context.resolution.x_dpi.max(1), context.resolution.y_dpi.max(1));
        let gray = !context.colour;
        let mut pages = Vec::with_capacity(count);
        for index in 0..count {
            if context.cancel.is_cancelled() {
                return Err(PrintError::new(ErrorKind::Cancelled, "the print job was cancelled while the document was rendered"));
            }
            pages.push(render_page(document.bytes, index, (dx, dy), gray)?);
        }
        let duplex = match context.duplex {
            None | Some(Duplex::Off) => pwg::Duplex::Off,
            Some(Duplex::LongEdge) => pwg::Duplex::LongEdge,
            Some(Duplex::ShortEdge) => pwg::Duplex::ShortEdge,
        };
        let raster = pwg::encode(&pages, &pwg::PwgOptions { dpi: (dx, dy), duplex }).map_err(|e| failed(e.to_string()))?;
        Ok(OwnedDocument::new(DocumentFormat::PwgRaster, raster))
    }
}

/// One page as a raster page at `dpi` (the renderer takes one scale, so the finer axis is rendered
/// and the other is sampled down).
fn render_page(bytes: &[u8], index: usize, (dx, dy): (u32, u32), gray: bool) -> Result<pwg::RasterPage, PrintError> {
    let number = index + 1;
    let (w, h) = designcraft_render::pdf_page_size(bytes, index).ok_or_else(|| failed(format!("page {number} could not be read")))?;
    let top = dx.max(dy);
    let scale = f64::from(top) / 72.0;
    // A raster header that says otherwise than the pixels would print at the wrong size, so a page
    // the renderer would shrink is refused.
    if !(w.is_finite() && h.is_finite()) || w <= 0.0 || h <= 0.0 || scale > MAX_SCALE {
        return Err(failed(format!("page {number} is too large to render at {top} dpi: lower the resolution")));
    }
    let side = (w.max(h) * scale).round().clamp(1.0, 65_000.0) as u32;
    let pm = designcraft_render::render_pdf_page(bytes, index, side).ok_or_else(|| failed(format!("page {number} could not be rendered")))?;
    let (rw, rh) = (u32::from(pm.width()), u32::from(pm.height()));
    let bpp = if gray { 1 } else { 3 };
    let flat = flatten(pm.data().iter().map(|p| [p.r, p.g, p.b, p.a]), gray);
    if rw == 0 || rh == 0 || flat.len() != rw as usize * rh as usize * bpp {
        return Err(failed(format!("page {number} was rendered with an unexpected size")));
    }
    if dx == dy {
        return Ok(pwg::RasterPage { width: rw, height: rh, gray, pixels: flat });
    }
    let target = |points: f64, dpi: u32| ((points * f64::from(dpi) / 72.0).ceil() as u32).max(1);
    let (tw, th) = (target(w, dx), target(h, dy));
    Ok(pwg::RasterPage { width: tw, height: th, gray, pixels: resample(&flat, bpp, (rw, rh), (tw, th)) })
}

/// Nearest-neighbour sampling of an interleaved 8-bit picture.
fn resample(pixels: &[u8], bpp: usize, (sw, sh): (u32, u32), (tw, th): (u32, u32)) -> Vec<u8> {
    let mut out = Vec::with_capacity(tw as usize * th as usize * bpp);
    for y in 0..th {
        let sy = (u64::from(y) * u64::from(sh) / u64::from(th)).min(u64::from(sh) - 1) as usize;
        for x in 0..tw {
            let sx = (u64::from(x) * u64::from(sw) / u64::from(tw)).min(u64::from(sw) - 1) as usize;
            let at = (sy * sw as usize + sx) * bpp;
            out.extend_from_slice(pixels.get(at..at + bpp).unwrap_or(&[]));
        }
    }
    out
}

/// Premultiplied RGBA over white, as 8-bit grey (Rec. 709 luma) or RGB.
fn flatten(rgba: impl Iterator<Item = [u8; 4]>, gray: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for px in rgba {
        let a = u16::from(px[3]);
        let over = |c: u8| (u16::from(c) + (255 - a)).min(255) as u8;
        let (r, g, b) = (over(px[0]), over(px[1]), over(px[2]));
        if gray {
            out.push(((u32::from(r) * 2126 + u32::from(g) * 7152 + u32::from(b) * 722 + 5000) / 10_000) as u8);
        } else {
            out.extend_from_slice(&[r, g, b]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flattening_composites_over_white() {
        assert_eq!(flatten([[0, 0, 0, 255], [0, 0, 0, 0]].into_iter(), false), [0, 0, 0, 255, 255, 255]);
        assert_eq!(flatten([[255, 255, 255, 255]].into_iter(), true), [255]);
        assert_eq!(flatten([[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]].into_iter(), true), [54, 182, 18]);
    }

    #[test]
    fn resampling_keeps_rows_and_columns_in_place() {
        assert_eq!(resample(&[1, 2, 3, 4], 1, (2, 2), (4, 1)), [1, 1, 2, 2]);
        assert_eq!(resample(&[1, 2, 3, 4], 1, (2, 2), (1, 2)), [1, 3]);
    }

    #[test]
    fn only_pdf_to_raster_is_offered() {
        assert_eq!(PdfToRaster.targets(DocumentFormat::Pdf), [DocumentFormat::PwgRaster]);
        assert!(PdfToRaster.targets(DocumentFormat::Jpeg).is_empty());
    }
}
