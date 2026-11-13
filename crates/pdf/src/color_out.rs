//! Export PDF › Output: colour conversion to a CMYK destination. Vector colours go through the
//! colour settings exactly; rasters through lookup tables (sRGB → CMYK over a 33³ grid,
//! working CMYK → destination CMYK over an 11⁴ grid), so a page of photos converts in well under
//! a second.

use std::sync::{Arc, OnceLock};

use designcraft_color::Color;
use designcraft_color::cms::{Cms, ColorSettings, Intent};
use designcraft_images::CmykRaster;

use crate::{ColorConversion, Output, PdfError};

/// Grid points per axis of the RGB → CMYK table.
const RGB_GRID: usize = 33;
/// Grid points per axis of the CMYK → CMYK table.
const CMYK_GRID: usize = 11;

/// Converts an export's colours into the destination CMYK space.
pub(crate) struct CmykConverter {
    /// The document's working spaces: where its colours are defined.
    source: Arc<Cms>,
    /// The working spaces with the destination as CMYK.
    dest: Cms,
    intent: Intent,
    /// Whether CMYK is converted rather than keeping its numbers (see [`Self::converts_cmyk`]).
    convert_cmyk: bool,
    /// `RGB_GRID³` CMYK samples over sRGB, red fastest; built for the first raster, so an export
    /// without rasters never pays for it.
    rgb_lut: OnceLock<Vec<[u8; 4]>>,
    /// `CMYK_GRID⁴` destination samples over the working CMYK, cyan fastest; built for the first
    /// CMYK raster when CMYK is converted.
    cmyk_lut: OnceLock<Vec<[u8; 4]>>,
}

impl CmykConverter {
    /// The converter for `output`, `None` without a conversion. An unknown destination is an
    /// error, not a silent fallback: the PDF would carry other inks than asked for.
    pub(crate) fn for_output(output: &Output) -> Result<Option<Arc<Self>>, PdfError> {
        Self::with_source(output, designcraft_color::cms::active())
    }

    pub(crate) fn with_source(output: &Output, source: Arc<Cms>) -> Result<Option<Arc<Self>>, PdfError> {
        if output.conversion == ColorConversion::None {
            return Ok(None);
        }
        let settings: ColorSettings = source.settings().clone();
        let cmyk = match output.destination.as_deref().map(str::trim) {
            Some(d) if !d.is_empty() => d.to_string(),
            _ => settings.cmyk.clone(),
        };
        let dest = Cms::new(&ColorSettings { cmyk: cmyk.clone(), ..settings.clone() })
            .map_err(|e| PdfError::Write(format!("Output: the destination `{cmyk}` can't be used: {e}")))?;
        let convert_cmyk = output.conversion == ColorConversion::ToDestination && cmyk != settings.cmyk;
        Ok(Some(Arc::new(Self { source, dest, intent: settings.intent, convert_cmyk, rgb_lut: OnceLock::new(), cmyk_lut: OnceLock::new() })))
    }

    /// The sRGB → destination table, built on first use.
    fn rgb_table(&self) -> &[[u8; 4]] {
        self.rgb_lut.get_or_init(|| {
            let step = 1.0 / (RGB_GRID - 1) as f32;
            let mut lut = Vec::with_capacity(RGB_GRID.pow(3));
            for b in 0..RGB_GRID {
                for g in 0..RGB_GRID {
                    for r in 0..RGB_GRID {
                        lut.push(self.dest.srgb_to_cmyk([r as f32 * step, g as f32 * step, b as f32 * step], self.intent).map(to_byte));
                    }
                }
            }
            lut
        })
    }

    /// The working CMYK → destination table, built on first use.
    fn cmyk_table(&self) -> &[[u8; 4]] {
        self.cmyk_lut.get_or_init(|| {
            let step = 1.0 / (CMYK_GRID - 1) as f32;
            let mut lut = Vec::with_capacity(CMYK_GRID.pow(4));
            for k in 0..CMYK_GRID {
                for y in 0..CMYK_GRID {
                    for m in 0..CMYK_GRID {
                        for c in 0..CMYK_GRID {
                            let inks = [c, m, y, k].map(|v| v as f32 * step);
                            lut.push(self.dest.lab_to_cmyk(self.source.cmyk_to_lab(inks), self.intent).map(to_byte));
                        }
                    }
                }
            }
            lut
        })
    }

    /// The destination's colour management: its CMYK is the output intent and blending space.
    pub(crate) fn destination(&self) -> &Cms {
        &self.dest
    }

    /// Whether CMYK is converted (Convert to Destination into another profile than the working
    /// CMYK) rather than keeping its numbers.
    pub(crate) fn converts_cmyk(&self) -> bool {
        self.convert_cmyk
    }

    /// A vector colour as the destination prints it: RGB through the profiles; CMYK converted
    /// or kept (see [`Self::converts_cmyk`]); grey keeps its value.
    pub(crate) fn color(&self, c: &Color) -> Color {
        match *c {
            Color::Rgb { .. } => {
                let [cy, m, y, k] = self.dest.srgb_to_cmyk(self.source.display_rgb(c), self.intent);
                Color::cmyk(cy, m, y, k)
            }
            Color::Cmyk { c, m, y, k } if self.converts_cmyk() => {
                let [cy, m, y, k] = self.dest.lab_to_cmyk(self.source.cmyk_to_lab([c, m, y, k]), self.intent);
                Color::cmyk(cy, m, y, k)
            }
            other => other,
        }
    }

    /// One sRGB pixel through the table (trilinear).
    fn rgb_pixel(lut: &[[u8; 4]], rgb: [u8; 3]) -> [u8; 4] {
        let at = |i: [usize; 3]| lut.get((i[2] * RGB_GRID + i[1]) * RGB_GRID + i[0]).copied().unwrap_or([0; 4]);
        interpolate::<3>(rgb, RGB_GRID, at)
    }

    /// One working-CMYK pixel through the table (quadrilinear).
    fn cmyk_pixel(lut: &[[u8; 4]], inks: [u8; 4]) -> [u8; 4] {
        let g = CMYK_GRID;
        let at = |i: [usize; 4]| lut.get(((i[3] * g + i[2]) * g + i[1]) * g + i[0]).copied().unwrap_or([0; 4]);
        interpolate::<4>(inks, g, at)
    }

    /// A straight-alpha sRGB raster as destination inks with the same alpha (`None` when opaque).
    pub(crate) fn rgb_raster(&self, rgba: &image::RgbaImage) -> CmykRaster {
        let (width, height) = rgba.dimensions();
        let lut = self.rgb_table();
        let mut cmyk = Vec::with_capacity(rgba.as_raw().len());
        for px in rgba.pixels() {
            cmyk.extend(Self::rgb_pixel(lut, [px[0], px[1], px[2]]));
        }
        // An alpha channel only when a pixel is less than opaque.
        let alpha = rgba.pixels().any(|px| px[3] < 255).then(|| rgba.pixels().map(|px| px[3]).collect());
        CmykRaster { width, height, cmyk, alpha }
    }

    /// A working-CMYK raster in the destination: converted, or as it is when CMYK keeps its
    /// numbers.
    pub(crate) fn cmyk_raster(&self, mut r: CmykRaster) -> CmykRaster {
        if self.converts_cmyk() {
            let lut = self.cmyk_table();
            for px in r.cmyk.as_chunks_mut::<4>().0 {
                *px = Self::cmyk_pixel(lut, *px);
            }
        }
        r
    }
}

/// A 0–1 ink as a byte.
fn to_byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// `N`-linear interpolation of a table of `grid` points per axis over 0–255 inputs; `at` reads
/// the sample at grid indices.
fn interpolate<const N: usize>(v: [u8; N], grid: usize, at: impl Fn([usize; N]) -> [u8; 4]) -> [u8; 4] {
    let scale = (grid - 1) as f32 / 255.0;
    let mut base = [0usize; N];
    let mut frac = [0f32; N];
    for d in 0..N {
        let x = f32::from(v[d]) * scale;
        let i = (x.floor() as usize).min(grid - 2);
        base[d] = i;
        frac[d] = x - i as f32;
    }
    let mut sum = [0f32; 4];
    for corner in 0..(1usize << N) {
        let mut idx = base;
        let mut w = 1.0;
        for d in 0..N {
            if corner >> d & 1 == 1 {
                idx[d] += 1;
                w *= frac[d];
            } else {
                w *= 1.0 - frac[d];
            }
        }
        if w > 0.0 {
            for (s, c) in sum.iter_mut().zip(at(idx)) {
                *s += w * f32::from(c);
            }
        }
    }
    sum.map(|s| s.round().clamp(0.0, 255.0) as u8)
}

/// How this export writes colours: CMYK → RGB (PDF/A without a CMYK output profile), into the
/// destination CMYK (Output), or each in its own space.
#[derive(Clone, Copy)]
pub(crate) struct ColorOut<'a> {
    pub rgb_only: bool,
    pub to_cmyk: Option<&'a CmykConverter>,
}

impl ColorOut<'_> {
    /// The colour as this export writes it (before PDF/A's CMYK → RGB).
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

    fn converter(conversion: ColorConversion, destination: Option<&str>) -> Arc<CmykConverter> {
        let output = Output { conversion, destination: destination.map(str::to_string) };
        CmykConverter::with_source(&output, Arc::new(Cms::default())).unwrap().unwrap()
    }

    #[test]
    fn rgb_becomes_cmyk_and_cmyk_keeps_its_numbers_in_its_own_space() {
        let conv = converter(ColorConversion::ToDestination, None);
        assert!(!conv.converts_cmyk(), "the destination is the working CMYK");
        assert_eq!(CmykConverter::rgb_pixel(conv.rgb_table(), [255, 255, 255]), [0, 0, 0, 0]);
        let [c, m, y, k] = CmykConverter::rgb_pixel(conv.rgb_table(), [0, 0, 0]);
        assert!(k > 200 && c > 0 && m > 0 && y > 0, "a rich black: {:?}", [c, m, y, k]);
        let red = conv.color(&Color::rgb(1.0, 0.0, 0.0));
        assert!(matches!(red, Color::Cmyk { c, m, y, .. } if c < 0.1 && m > 0.8 && y > 0.8), "{red:?}");
        // The table agrees with the exact conversion.
        let exact = conv.color(&Color::rgb(0.2, 0.6, 0.4));
        let Color::Cmyk { c, m, y, k } = exact else { panic!("{exact:?}") };
        let table = CmykConverter::rgb_pixel(conv.rgb_table(), [51, 153, 102]);
        for (t, e) in table.iter().zip([c, m, y, k]) {
            assert!((f32::from(*t) - e * 255.0).abs() <= 3.0, "{table:?} vs {exact:?}");
        }
        assert_eq!(conv.color(&Color::cmyk(0.1, 0.2, 0.3, 0.4)), Color::cmyk(0.1, 0.2, 0.3, 0.4));
        assert_eq!(conv.color(&Color::Gray { k: 0.5 }), Color::Gray { k: 0.5 });
        // A raster: opaque pixels give no alpha; a transparent one keeps it.
        let img = image::RgbaImage::from_fn(2, 1, |x, _| image::Rgba(if x == 0 { [255, 255, 255, 255] } else { [0, 0, 0, 128] }));
        let r = conv.rgb_raster(&img);
        assert_eq!((r.width, r.height, r.cmyk.get(..4)), (2, 1, Some(&[0, 0, 0, 0][..])));
        assert_eq!(r.alpha, Some(vec![255, 128]));
    }

    /// Convert to Destination converts working-space CMYK into another destination; Preserve
    /// Numbers keeps it. The two used to give the same file.
    #[test]
    fn destination_conversion_and_preserve_numbers_differ_for_cmyk() {
        let other = designcraft_color::cms::GENERIC_CMYK;
        let convert = converter(ColorConversion::ToDestination, Some(other));
        let keep = converter(ColorConversion::PreserveNumbers, Some(other));
        assert!(convert.converts_cmyk() && !keep.converts_cmyk());
        let ink = Color::cmyk(0.6, 0.3, 0.1, 0.05);
        assert_eq!(keep.color(&ink), ink);
        let moved = convert.color(&ink);
        assert_ne!(moved, ink, "FOGRA39 → generic CMYK moves the numbers");
        // Paper stays paper, and the table follows the exact conversion.
        assert_eq!(CmykConverter::cmyk_pixel(convert.cmyk_table(), [0, 0, 0, 0]), [0, 0, 0, 0]);
        let Color::Cmyk { c, m, y, k } = moved else { panic!("{moved:?}") };
        let table = CmykConverter::cmyk_pixel(convert.cmyk_table(), [153, 77, 26, 13]);
        for (t, e) in table.iter().zip([c, m, y, k]) {
            assert!((f32::from(*t) - e * 255.0).abs() <= 6.0, "{table:?} vs {moved:?}");
        }
        let raster = CmykRaster { width: 1, height: 1, cmyk: vec![153, 77, 26, 13], alpha: None };
        assert_eq!(keep.cmyk_raster(raster.clone()).cmyk, raster.cmyk);
        assert_eq!(convert.cmyk_raster(raster).cmyk, table.to_vec());
    }

    #[test]
    fn no_conversion_no_converter_and_unknown_destinations_are_errors() {
        let source = Arc::new(Cms::default());
        assert!(CmykConverter::with_source(&Output::default(), source.clone()).unwrap().is_none());
        let unknown = Output { conversion: ColorConversion::ToDestination, destination: Some("No Such Profile".into()) };
        assert!(CmykConverter::with_source(&unknown, source).is_err());
    }
}
