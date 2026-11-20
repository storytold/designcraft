//! DesignCraft's PDF/X-1a export adapter: convert shared warnings and errors to app PDF results.

#[cfg(test)]
use craft_prepress::flatten::FlattenTotals;

use crate::PdfError;

pub(crate) use craft_prepress::pdfx1a::PageGraphics;

pub(crate) fn flatten_x1a(pdf: &[u8], dpi: f32, pages: &[PageGraphics], warnings: &mut Vec<String>) -> crate::Result<Vec<u8>> {
    let (bytes, findings) = craft_prepress::pdfx1a::flatten_x1a(pdf, dpi, pages).map_err(PdfError::Write)?;
    warnings.extend(findings);
    Ok(bytes)
}

#[cfg(test)]
pub(crate) fn flatten_warnings(totals: &FlattenTotals, dpi: f32) -> Vec<String> {
    craft_prepress::pdfx1a::flatten_warnings(totals, dpi)
}

#[cfg(test)]
pub(crate) fn finish(pdf: &[u8]) -> Result<Vec<u8>, String> {
    craft_prepress::pdfx1a::finish_x1a(pdf)
}

pub fn check_pdfx1a(pdf: &[u8]) -> Vec<String> {
    craft_prepress::pdfx1a::check_pdfx1a(pdf)
}
