//! Export PDF › Output › Color Conversion: process colours and image pixels converted from the
//! working spaces to a destination profile.
//!
//! * **To a CMYK destination:** RGB becomes CMYK in the destination; working-space CMYK is
//!   re-separated for the destination (Convert to Destination) or keeps its numbers (Preserve
//!   Numbers, also when the destination is the working space). Gray is left alone: it is the
//!   black ink either way.
//! * **To an RGB destination:** CMYK becomes RGB in the destination; working-space RGB is
//!   converted (Convert to Destination) or keeps its numbers (Preserve Numbers). Gray is left
//!   alone.
//!
//! Spot colours are inks, not converted (their alternate values are). Images with an RGB
//! profile of their own are converted from that profile.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, PoisonError};

use designcraft_color::cms::{Cms, ColorSettings, IccProfile, Intent, Model, ProfileKind};

use super::sampling::Raster;
use super::*;
use crate::{ColorConversion, OutputColor};

/// Grid points per axis of the RGB → CMYK pixel table (trilinear between them).
const GRID: usize = 33;

pub(crate) struct Converter {
    /// The working spaces colours are defined in.
    source: Cms,
    /// The working spaces with the destination in place of its model's.
    dest: Cms,
    pub(crate) target: Model,
    /// Colours already in the target model keep their numbers.
    preserve: bool,
    intent: Intent,
    /// The destination's name, for warnings and the output intent.
    pub(crate) destination: String,
    /// sRGB → destination CMYK on a grid, built on first use.
    rgb_grid: OnceLock<Vec<[f32; 4]>>,
    /// Working CMYK → destination, by ink values (bounded).
    cmyk_memo: Mutex<HashMap<[u8; 4], [u8; 4]>>,
}

/// Most entries kept in a pixel memo before converting without it.
const MEMO_LIMIT: usize = 1 << 20;

impl Converter {
    /// The conversion `out` asks for with the working spaces `working`, or `None` when it
    /// changes nothing. `print` picks CMYK as the destination model when `out` names no
    /// profile. An unknown profile is an error naming it.
    pub(crate) fn new(out: &OutputColor, working: &ColorSettings, print: bool) -> std::result::Result<Option<Self>, String> {
        if out.conversion == ColorConversion::None {
            return Ok(None);
        }
        let (target, name) = match &out.destination {
            Some(name) => {
                let kind = designcraft_color::cms::profiles()
                    .into_iter()
                    .find(|p| p.name == *name)
                    .map(|p| p.kind)
                    .ok_or_else(|| format!("unknown destination profile `{name}`"))?;
                match kind {
                    ProfileKind::Cmyk => (Model::Cmyk, name.clone()),
                    // Vector colours are written as DeviceRGB (sRGB): another RGB profile would
                    // describe images but not fills, which would then disagree.
                    ProfileKind::Rgb if name != designcraft_color::cms::SRGB => {
                        return Err(format!(
                            "`{name}`: the RGB destination is sRGB (another RGB profile can't describe vector colours); pick sRGB or a CMYK profile"
                        ));
                    }
                    ProfileKind::Rgb => (Model::Rgb, name.clone()),
                    ProfileKind::Gray => return Err(format!("`{name}` is a grayscale profile; pick an RGB or CMYK destination")),
                }
            }
            None if print => (Model::Cmyk, working.cmyk.clone()),
            None => (Model::Rgb, designcraft_color::cms::SRGB.to_string()),
        };
        let source = Cms::new(working).map_err(|e| e.to_string())?;
        let mut d = working.clone();
        match target {
            Model::Cmyk => d.cmyk = name.clone(),
            _ => d.rgb = name.clone(),
        }
        let dest = Cms::new(&d).map_err(|e| e.to_string())?;
        // Converting a model to itself in the same space is preserving its numbers.
        let same = match target {
            Model::Cmyk => working.cmyk == name,
            _ => working.rgb == name,
        };
        Ok(Some(Self {
            source,
            dest,
            target,
            preserve: out.conversion == ColorConversion::PreserveNumbers || same,
            intent: out.intent.unwrap_or(working.intent),
            destination: name,
            rgb_grid: OnceLock::new(),
            cmyk_memo: Mutex::new(HashMap::new()),
        }))
    }

    /// A process colour in the destination.
    pub(crate) fn color(&self, c: &Color) -> Color {
        match (*c, self.target) {
            (Color::Gray { .. }, _) => *c,
            (Color::Rgb { r, g, b }, Model::Cmyk) => {
                let [c, m, y, k] = self.dest.srgb_to_cmyk(self.source.rgb_to_srgb([r, g, b]), self.intent);
                Color::cmyk(c, m, y, k)
            }
            (Color::Cmyk { .. }, Model::Cmyk) if self.preserve => *c,
            (Color::Cmyk { c, m, y, k }, Model::Cmyk) => {
                let [c, m, y, k] = self.dest.lab_to_cmyk(self.source.cmyk_to_lab([c, m, y, k]), self.intent);
                Color::cmyk(c, m, y, k)
            }
            (Color::Rgb { .. }, _) if self.preserve => *c,
            (Color::Rgb { r, g, b }, _) => {
                let [r, g, b] = self.dest.srgb_to_rgb(self.source.rgb_to_srgb([r, g, b]));
                Color::rgb(r, g, b)
            }
            (Color::Cmyk { c, m, y, k }, _) => {
                let [r, g, b] = self.dest.srgb_to_rgb(self.source.cmyk_to_srgb([c, m, y, k], false));
                Color::rgb(r, g, b)
            }
        }
    }

    /// Whether an image of this raster's model is changed by the conversion.
    pub(crate) fn changes(&self, r: &Raster) -> bool {
        match (r, self.target) {
            (Raster::Grey { .. }, _) => false,
            (Raster::Rgb { .. }, Model::Cmyk) => true,
            (Raster::Cmyk(_), Model::Cmyk) => !self.preserve,
            (Raster::Rgb { icc, .. }, _) => !self.preserve || icc.is_some(),
            (Raster::Cmyk(_), _) => true,
        }
    }

    /// Whether a JPEG with `components` channels is changed (without decoding it). `own_rgb`:
    /// it carries an RGB profile of its own, which is converted from even when numbers are
    /// preserved, as for other images.
    pub(crate) fn changes_jpeg(&self, components: u8, own_rgb: bool) -> bool {
        match (components, self.target) {
            (1, _) => false,
            (4, Model::Cmyk) => !self.preserve,
            (4, _) => true,
            (_, Model::Cmyk) => true,
            _ => !self.preserve || own_rgb,
        }
    }

    /// The raster's pixels in the destination. Alpha is kept.
    pub(crate) fn raster(&self, r: Raster) -> Option<Raster> {
        if !self.changes(&r) {
            return Some(r);
        }
        Some(match (r, self.target) {
            (Raster::Rgb { rgba, icc }, Model::Cmyk) => {
                let own = icc.as_deref().and_then(|p| IccProfile::from_bytes(None, p).ok()).filter(|p| p.kind == ProfileKind::Rgb);
                let (w, h) = rgba.dimensions();
                let pixels = (w as usize).checked_mul(h as usize)?;
                let mut cmyk = Vec::with_capacity(pixels.checked_mul(4)?);
                let mut alpha = Vec::with_capacity(pixels);
                // An image with its own profile gets its own table.
                let own_grid;
                let grid: &[[f32; 4]] = match &own {
                    Some(p) => {
                        own_grid = self.grid_from(|rgb| p.to_srgb(&rgb, Intent::RelativeColorimetric, true).unwrap_or(rgb));
                        &own_grid
                    }
                    None => self.rgb_grid.get_or_init(|| self.grid_from(|rgb| self.source.rgb_to_srgb(rgb))),
                };
                for p in rgba.pixels() {
                    let v = lookup(grid, p.0[0], p.0[1], p.0[2]);
                    cmyk.extend(v.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8));
                    alpha.push(p.0[3]);
                }
                let alpha = alpha.iter().any(|a| *a < 255).then_some(alpha);
                Raster::Cmyk(designcraft_images::CmykRaster { width: w, height: h, cmyk, alpha })
            }
            (Raster::Cmyk(mut c), Model::Cmyk) => {
                let mut memo = self.cmyk_memo.lock().unwrap_or_else(PoisonError::into_inner);
                for px in c.cmyk.as_chunks_mut::<4>().0.iter_mut() {
                    let key = *px;
                    let v = match memo.get(&key) {
                        Some(v) => *v,
                        None => {
                            let v = self.cmyk_pixel(key);
                            if memo.len() < MEMO_LIMIT {
                                memo.insert(key, v);
                            }
                            v
                        }
                    };
                    *px = v;
                }
                Raster::Cmyk(c)
            }
            (Raster::Cmyk(c), _) => {
                let w = c.width;
                let h = c.height;
                let rgba = image::RgbaImage::from_fn(w, h, |x, y| {
                    let i = (y as usize * w as usize + x as usize) * 4;
                    let px = c.cmyk.get(i..i + 4).unwrap_or(&[0, 0, 0, 0]);
                    let srgb = self.source.cmyk_to_srgb(px_to_unit(px), false);
                    let [r, g, b] = self.dest.srgb_to_rgb(srgb).map(unit_to_u8);
                    let a = c.alpha.as_ref().and_then(|a| a.get(y as usize * w as usize + x as usize).copied()).unwrap_or(255);
                    image::Rgba([r, g, b, a])
                });
                Raster::Rgb { rgba, icc: self.dest_rgb_icc() }
            }
            (Raster::Rgb { mut rgba, icc }, _) => {
                let own = icc.as_deref().and_then(|p| IccProfile::from_bytes(None, p).ok()).filter(|p| p.kind == ProfileKind::Rgb);
                let mut memo: HashMap<[u8; 3], [u8; 3]> = HashMap::new();
                for p in rgba.pixels_mut() {
                    let key = [p.0[0], p.0[1], p.0[2]];
                    let v = match memo.get(&key) {
                        Some(v) => *v,
                        None => {
                            let rgb = key.map(|v| f32::from(v) / 255.0);
                            let srgb = match &own {
                                Some(prof) => prof.to_srgb(&rgb, Intent::RelativeColorimetric, true).unwrap_or(rgb),
                                None => self.source.rgb_to_srgb(rgb),
                            };
                            let v = self.dest.srgb_to_rgb(srgb).map(unit_to_u8);
                            if memo.len() < MEMO_LIMIT {
                                memo.insert(key, v);
                            }
                            v
                        }
                    };
                    p.0[..3].copy_from_slice(&v);
                }
                Raster::Rgb { rgba, icc: self.dest_rgb_icc() }
            }
            (other, _) => other,
        })
    }

    fn cmyk_pixel(&self, px: [u8; 4]) -> [u8; 4] {
        let [c, m, y, k] = px_to_unit(&px);
        self.dest.lab_to_cmyk(self.source.cmyk_to_lab([c, m, y, k]), self.intent).map(unit_to_u8)
    }

    /// The RGB destination's profile, to tag converted RGB images with.
    fn dest_rgb_icc(&self) -> Option<Arc<Vec<u8>>> {
        if self.destination == designcraft_color::cms::SRGB {
            return Some(Arc::new(super::SRGB_ICC.to_vec()));
        }
        designcraft_color::cms::rgb_profile_bytes(&self.destination).map(Arc::new)
    }

    /// `to_srgb` then sRGB → destination CMYK, at every grid point.
    fn grid_from(&self, to_srgb: impl Fn([f32; 3]) -> [f32; 3]) -> Vec<[f32; 4]> {
        let step = 1.0 / (GRID - 1) as f32;
        let mut out = Vec::with_capacity(GRID * GRID * GRID);
        for r in 0..GRID {
            for g in 0..GRID {
                for b in 0..GRID {
                    let rgb = [r as f32 * step, g as f32 * step, b as f32 * step];
                    out.push(self.dest.srgb_to_cmyk(to_srgb(rgb), self.intent));
                }
            }
        }
        out
    }
}

fn px_to_unit(px: &[u8]) -> [f32; 4] {
    let g = |i: usize| f32::from(px.get(i).copied().unwrap_or(0)) / 255.0;
    [g(0), g(1), g(2), g(3)]
}

fn unit_to_u8(v: f32) -> u8 {
    if v.is_finite() { (v.clamp(0.0, 1.0) * 255.0).round() as u8 } else { 0 }
}

/// Trilinear lookup in a [`GRID`]³ table.
fn lookup(grid: &[[f32; 4]], r: u8, g: u8, b: u8) -> [f32; 4] {
    let scale = (GRID - 1) as f32 / 255.0;
    let pos = |v: u8| {
        let f = f32::from(v) * scale;
        let i = (f.floor() as usize).min(GRID - 2);
        (i, f - i as f32)
    };
    let ((ri, rf), (gi, gf), (bi, bf)) = (pos(r), pos(g), pos(b));
    let at = |i: usize, j: usize, k: usize| grid.get((i * GRID + j) * GRID + k).copied().unwrap_or([0.0; 4]);
    let mut out = [0f32; 4];
    for (di, wr) in [(0, 1.0 - rf), (1, rf)] {
        for (dj, wg) in [(0, 1.0 - gf), (1, gf)] {
            for (dk, wb) in [(0, 1.0 - bf), (1, bf)] {
                let w = wr * wg * wb;
                if w == 0.0 {
                    continue;
                }
                let v = at(ri + di, gi + dj, bi + dk);
                for (o, c) in out.iter_mut().zip(v) {
                    *o += c * w;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_color::cms::{COATED_FOGRA39, GENERIC_CMYK, SRGB};

    fn out(conversion: ColorConversion, destination: Option<&str>) -> OutputColor {
        OutputColor { conversion, destination: destination.map(str::to_string), include_profile: true, intent: None }
    }

    #[test]
    fn no_conversion_is_no_converter() {
        assert!(Converter::new(&out(ColorConversion::None, Some(GENERIC_CMYK)), &ColorSettings::default(), true).unwrap().is_none());
    }

    #[test]
    fn other_rgb_destinations_than_srgb_are_errors() {
        let o = out(ColorConversion::ToDestination, Some(designcraft_color::cms::DISPLAY_P3));
        let e = Converter::new(&o, &ColorSettings::default(), false).err().unwrap();
        assert!(e.contains("sRGB"), "{e}");
    }

    #[test]
    fn unknown_or_grey_destinations_are_errors() {
        let e = Converter::new(&out(ColorConversion::ToDestination, Some("No Such Profile")), &ColorSettings::default(), true).err().unwrap();
        assert!(e.contains("No Such Profile"), "{e}");
    }

    #[test]
    fn rgb_becomes_cmyk_and_cmyk_keeps_numbers_in_the_working_space() {
        let cv = Converter::new(&out(ColorConversion::ToDestination, None), &ColorSettings::default(), true).unwrap().unwrap();
        assert_eq!(cv.target, Model::Cmyk);
        assert_eq!(cv.destination, COATED_FOGRA39);
        assert!(matches!(cv.color(&Color::rgb(1.0, 0.0, 0.0)), Color::Cmyk { .. }));
        // Same space: numbers kept even when converting.
        let c = Color::cmyk(0.1, 0.2, 0.3, 0.4);
        assert_eq!(cv.color(&c), c);
        assert_eq!(cv.color(&Color::Gray { k: 0.5 }), Color::Gray { k: 0.5 });
    }

    #[test]
    fn another_cmyk_destination_re_separates_unless_numbers_are_preserved() {
        let c = Color::cmyk(0.6, 0.1, 0.0, 0.0);
        let cv = Converter::new(&out(ColorConversion::ToDestination, Some(GENERIC_CMYK)), &ColorSettings::default(), true).unwrap().unwrap();
        assert!(matches!(cv.color(&c), Color::Cmyk { .. }));
        assert_ne!(cv.color(&c), c);
        let keep = Converter::new(&out(ColorConversion::PreserveNumbers, Some(GENERIC_CMYK)), &ColorSettings::default(), true).unwrap().unwrap();
        assert_eq!(keep.color(&c), c);
        assert!(matches!(keep.color(&Color::rgb(0.0, 0.5, 1.0)), Color::Cmyk { .. }));
    }

    #[test]
    fn rgb_destination_converts_cmyk() {
        let cv = Converter::new(&out(ColorConversion::ToDestination, Some(SRGB)), &ColorSettings::default(), false).unwrap().unwrap();
        assert_eq!(cv.target, Model::Rgb);
        assert!(matches!(cv.color(&Color::cmyk(0.0, 0.0, 0.0, 1.0)), Color::Rgb { .. }));
        let rgb = Color::rgb(0.2, 0.4, 0.6);
        // sRGB to sRGB is the same space.
        assert_eq!(cv.color(&rgb), rgb);
    }

    #[test]
    fn rgb_pixels_follow_the_colour_conversion() {
        let cv = Converter::new(&out(ColorConversion::ToDestination, None), &ColorSettings::default(), true).unwrap().unwrap();
        let rgba = image::RgbaImage::from_fn(3, 1, |x, _| {
            [image::Rgba([255, 0, 0, 255]), image::Rgba([255, 255, 255, 128]), image::Rgba([0, 0, 0, 255])][x as usize]
        });
        let Some(Raster::Cmyk(c)) = cv.raster(Raster::Rgb { rgba, icc: None }) else { panic!("cmyk") };
        assert_eq!((c.width, c.height), (3, 1));
        // The table agrees with converting the colour directly.
        let Color::Cmyk { c: c0, m: m0, .. } = cv.color(&Color::rgb(1.0, 0.0, 0.0)) else { panic!() };
        assert!((i32::from(c.cmyk[0]) - i32::from(unit_to_u8(c0))).abs() <= 2);
        assert!((i32::from(c.cmyk[1]) - i32::from(unit_to_u8(m0))).abs() <= 2);
        // White is no ink; alpha is kept.
        assert!(c.cmyk[4..8].iter().all(|v| *v <= 2), "{:?}", &c.cmyk[4..8]);
        assert_eq!(c.alpha.as_deref(), Some([255u8, 128, 255].as_slice()));
        // Grey images are left alone.
        let grey = Raster::Grey { la: image::GrayAlphaImage::new(1, 1), icc: None };
        assert!(!cv.changes(&grey));
    }

    /// The rendering intent chosen for the export wins over Color Settings'.
    #[test]
    fn the_export_intent_drives_the_conversion() {
        let saturated = Color::rgb(0.0, 1.0, 0.2);
        let with = |intent| {
            let o = OutputColor { intent, ..out(ColorConversion::ToDestination, Some(GENERIC_CMYK)) };
            Converter::new(&o, &ColorSettings::default(), true).unwrap().unwrap()
        };
        assert_eq!(with(None).intent, ColorSettings::default().intent);
        assert_eq!(with(Some(Intent::Perceptual)).intent, Intent::Perceptual);
        // Each intent is a real conversion of the colour into the destination.
        for i in Intent::ALL {
            assert!(matches!(with(Some(i)).color(&saturated), Color::Cmyk { .. }), "{i:?}");
        }
    }

    #[test]
    fn jpeg_channel_counts_decide_what_changes() {
        let to_cmyk = Converter::new(&out(ColorConversion::PreserveNumbers, Some(GENERIC_CMYK)), &ColorSettings::default(), true).unwrap().unwrap();
        assert!(to_cmyk.changes_jpeg(3, false));
        assert!(!to_cmyk.changes_jpeg(4, false));
        assert!(!to_cmyk.changes_jpeg(1, false));
        // To sRGB, numbers kept: only a JPEG tagged with another RGB profile is converted.
        let o = out(ColorConversion::PreserveNumbers, Some(designcraft_color::cms::SRGB));
        let to_srgb = Converter::new(&o, &ColorSettings::default(), false).unwrap().unwrap();
        assert!(!to_srgb.changes_jpeg(3, false));
        assert!(to_srgb.changes_jpeg(3, true));
    }
}
