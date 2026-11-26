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

/// Check PDF/X-1a, including the identification in PDFs with catalog updates.
pub fn check_pdfx1a(pdf: &[u8]) -> Vec<String> {
    const VERSION_MISSING: &str = "the document information doesn't name PDF/X-1:2003";
    const CONFORMANCE_MISSING: &str = "the document information doesn't name the conformance level PDF/X-1a:2003";
    let mut findings = craft_prepress::pdfx1a::check_pdfx1a(pdf);
    if !findings.iter().any(|finding| matches!(finding.as_str(), VERSION_MISSING | CONFORMANCE_MISSING)) {
        return findings;
    }
    let Ok(file) = craft_pdf::syntax::Pdf::new(pdf.to_vec()) else { return findings };

    // The shared identification check reads a single-revision trailer. Catalog updates
    // append a revision, so confirm these two fields through the parsed trailer chain.
    findings.retain(|finding| match finding.as_str() {
        VERSION_MISSING => crate::pdfx::info_text(pdf, &file, b"GTS_PDFXVersion").as_deref() != Some("PDF/X-1:2003"),
        CONFORMANCE_MISSING => crate::pdfx::info_text(pdf, &file, b"GTS_PDFXConformance").as_deref() != Some("PDF/X-1a:2003"),
        _ => true,
    });
    findings
}
