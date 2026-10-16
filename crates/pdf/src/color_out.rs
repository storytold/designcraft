//! Export PDF › Output: colour conversion to a CMYK destination (InDesign's "Convert to
//! Destination"). Vector colours go through the colour settings exactly; rasters through a
//! 33³ sRGB → CMYK lookup table with trilinear interpolation, so a page of photos converts in
//! well under a second.

use std::sync::Arc;

use designcraft_color::Color;
use designcraft_color::cms::{Cms, Intent};
use designcraft_images::CmykRaster;

use crate::{ColorConversion, Output, PdfError};

/// Grid points per axis of the raster lookup table.
const GRID: usize = 33;

/// RGB → CMYK through the destination profile.
pub(crate) struct CmykConverter {
    cms: Cms,
    intent: Intent,
    /// `GRID³` CMYK samples (0–255 each) over sRGB, red fastest.
    lut: Vec<[u8; 4]>,
}

impl CmykConverter {
    /// A converter for the export's Output options: the destination CMYK profile (the working
    /// CMYK without one) with the active rendering intent. `None` without conversion.
    pub(crate) fn for_output(output: &Output) -> Result<Option<Arc<Self>>, PdfError> {
        if output.conversion == ColorConversion::None {
            return Ok(None);
        }
        let mut settings = designcraft_color::cms::active_settings();
        if let Some(dest) = output.destination.as_deref().filter(|d| !d.trim().is_empty()) {
            settings.cmyk = dest.to_string();
        }
        let cms = Cms::new(&settings).map_err(|e| PdfError::Write(format!("colour conversion: {e}")))?;
        let intent = settings.intent;
        let step = 1.0 / (GRID - 1) as f32;
        let mut lut = Vec::with_capacity(GRID * GRID * GRID);
        for b in 0..GRID {
            for g in 0..GRID {
                for r in 0..GRID {
                    let cmyk = cms.srgb_to_cmyk([r as f32 * step, g as f32 * step, b as f32 * step], intent);
                    lut.push(cmyk.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
                }
            }
        }
        Ok(Some(Arc::new(Self { cms, intent, lut })))
    }

    /// A vector colour: RGB becomes CMYK through the profiles; CMYK and grey keep their numbers
    /// (InDesign's "Preserve Numbers"; untagged CMYK keeps them in both modes).
    pub(crate) fn color(&self, c: &Color) -> Color {
        match c {
            Color::Rgb { .. } => {
                let [cy, m, y, k] = self.cms.to_cmyk(c, self.intent);
                Color::cmyk(cy, m, y, k)
            }
            other => *other,
        }
    }

    /// One sRGB pixel through the table (trilinear).
    fn pixel(&self, r: u8, g: u8, b: u8) -> [u8; 4] {
        let scale = (GRID - 1) as f32 / 255.0;
        let pos = |v: u8| {
            let x = f32::from(v) * scale;
            let i = (x.floor() as usize).min(GRID - 2);
            (i, x - i as f32)
        };
        let ((ri, rf), (gi, gf), (bi, bf)) = (pos(r), pos(g), pos(b));
        let at = |ri: usize, gi: usize, bi: usize| self.lut.get((bi * GRID + gi) * GRID + ri).copied().unwrap_or([0; 4]);
        let mut out = [0u8; 4];
        for (ch, o) in out.iter_mut().enumerate() {
            let c = |ri: usize, gi: usize, bi: usize| f32::from(at(ri, gi, bi)[ch]);
            let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
            let r0 = lerp(lerp(c(ri, gi, bi), c(ri + 1, gi, bi), rf), lerp(c(ri, gi + 1, bi), c(ri + 1, gi + 1, bi), rf), gf);
            let r1 = lerp(lerp(c(ri, gi, bi + 1), c(ri + 1, gi, bi + 1), rf), lerp(c(ri, gi + 1, bi + 1), c(ri + 1, gi + 1, bi + 1), rf), gf);
            *o = lerp(r0, r1, bf).round().clamp(0.0, 255.0) as u8;
        }
        out
    }

    /// A straight-alpha sRGB raster as CMYK inks with the same alpha (`None` when opaque).
    pub(crate) fn raster(&self, rgba: &image::RgbaImage) -> CmykRaster {
        let (width, height) = rgba.dimensions();
        let mut cmyk = Vec::with_capacity(rgba.len());
        let mut alpha = Vec::with_capacity(rgba.len() / 4);
        let mut opaque = true;
        for px in rgba.pixels() {
            cmyk.extend(self.pixel(px[0], px[1], px[2]));
            alpha.push(px[3]);
            opaque &= px[3] == 255;
        }
        CmykRaster { width, height, cmyk, alpha: (!opaque).then_some(alpha) }
    }
}

/// How this export writes colours: CMYK → RGB (PDF/A without a CMYK output profile), RGB →
/// CMYK (Output › Convert to Destination), or each in its own space.
#[derive(Clone, Copy)]
pub(crate) struct ColorOut<'a> {
    pub rgb_only: bool,
    pub to_cmyk: Option<&'a CmykConverter>,
}

impl ColorOut<'_> {
    /// The colour as this export writes it.
    pub(crate) fn resolve(&self, c: &Color) -> Color {
        match self.to_cmyk {
            Some(conv) if !self.rgb_only => conv.color(c),
            _ => *c,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converter_maps_rgb_to_cmyk_and_keeps_cmyk() {
        let output = Output { conversion: ColorConversion::ToDestination, destination: None };
        let conv = CmykConverter::for_output(&output).unwrap().unwrap();
        // Pure white has no ink, pure black is mostly K, and CMYK numbers are untouched.
        assert_eq!(conv.pixel(255, 255, 255), [0, 0, 0, 0]);
        let [c, m, y, k] = conv.pixel(0, 0, 0);
        // A rich black through the generic profile: mostly K, with CMY under it.
        assert!(k > 200 && c < 200 && m < 200 && y < 200, "{:?}", [c, m, y, k]);
        let red = conv.color(&Color::rgb(1.0, 0.0, 0.0));
        assert!(matches!(red, Color::Cmyk { c, m, y, .. } if c < 0.1 && m > 0.8 && y > 0.8), "{red:?}");
        assert_eq!(conv.color(&Color::cmyk(0.1, 0.2, 0.3, 0.4)), Color::cmyk(0.1, 0.2, 0.3, 0.4));
        // A raster: opaque pixels give no alpha; a transparent one keeps it.
        let img = image::RgbaImage::from_fn(2, 1, |x, _| image::Rgba(if x == 0 { [255, 255, 255, 255] } else { [0, 0, 0, 128] }));
        let r = conv.raster(&img);
        assert_eq!((r.width, r.height), (2, 1));
        assert_eq!(&r.cmyk[..4], &[0, 0, 0, 0]);
        assert_eq!(r.alpha, Some(vec![255, 128]));
        // No conversion asked: no converter; an unknown destination is an error.
        assert!(CmykConverter::for_output(&Output::default()).unwrap().is_none());
        assert!(
            CmykConverter::for_output(&Output { conversion: ColorConversion::ToDestination, destination: Some("No Such Profile".into()) }).is_err()
        );
    }
}
