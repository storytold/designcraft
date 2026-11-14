//! DesignCraft's PDF/X-1a export adapter: convert shared warnings and errors to app PDF results.

#[cfg(test)]
use craft_prepress::flatten::FlattenTotals;

use crate::PdfError;

pub(crate) use craft_prepress::pdfx1a::PageGraphics;

/// RGB process colour into the output intent's CMYK with black point compensation, for PDF/X-1a
/// when the colour settings ask for it. The flattener converts the RGB left in the print file
/// without black point compensation, so this is made only when it is on: otherwise the export
/// stays the flattener's. Relative colorimetric, as the flattener converts.
pub(crate) struct ProcessConversion(std::sync::Arc<craft_cms::Transform>);

impl ProcessConversion {
    /// Into CMYK profile `icc` with black point compensation `bpc`; `None` without it, or when
    /// the profile isn't a CMYK profile craft-cms converts into (the flattener then converts).
    pub(crate) fn new(icc: &[u8], bpc: bool) -> Option<Self> {
        if !bpc {
            return None;
        }
        let profile = craft_cms::Profile::parse(icc).ok()?;
        if profile.color_space != craft_cms::ColorSpace::Cmyk {
            return None;
        }
        let opts = craft_cms::TransformOptions { intent: craft_cms::Intent::RelativeColorimetric, bpc, precise_float: false };
        craft_cms::cached(craft_cms::builtin::srgb(), &profile, opts).ok().map(Self)
    }

    /// The CMYK of 8-bit sRGB `rgb` (as the flattener reads DeviceRGB); `None` when it can't
    /// convert.
    pub(crate) fn cmyk(&self, rgb: [u8; 3]) -> Option<[f32; 4]> {
        let src = rgb.map(|v| f32::from(v) / 255.0);
        let mut out = [0.0f32; 4];
        self.0.convert_f32(&src, 3, &mut out, 4, false).ok()?;
        Some(out.map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }))
    }
}

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
