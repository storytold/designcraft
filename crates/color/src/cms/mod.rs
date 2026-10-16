//! Colour management: working spaces, RGB ↔ CMYK ↔ Lab conversions with rendering intents,
//! gamut checks and soft-proof transforms.
//!
//! * **RGB working spaces** are ICC matrix/shaper profiles via [`moxcms`] (sRGB, Adobe RGB (1998)
//!   compatible, Display P3, ProPhoto RGB) or user `.icc` files. Display output is sRGB.
//! * **CMYK spaces:** [`GENERIC_CMYK`] is our own documented parametric press model
//!   ([`generic`]); [`DEVICE_CMYK`] is the old profile-free formula (kept for exact legacy numbers);
//!   any CMYK `.icc` the user loads goes through `moxcms` (LUT-based profiles).
//! * The **active settings** (Edit → Color Settings) are process-wide; [`crate::Color::to_rgb`]
//!   uses them. Code that needs deterministic numbers builds its own [`Cms`] with [`Cms::new`].

pub mod generic;
pub mod icc;
pub mod icc_out;
pub mod lab;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use serde::{Deserialize, Serialize};

pub use generic::GenericCmyk;
pub use icc::IccProfile;
pub use lab::{Lab, delta_e76, delta_e2000};

use crate::Color;

pub const SRGB: &str = "sRGB IEC61966-2.1";
pub const ADOBE_RGB: &str = "Adobe RGB (1998) compatible";
pub const DISPLAY_P3: &str = "Display P3";
pub const PROPHOTO_RGB: &str = "ProPhoto RGB";
/// Our parametric press model (see [`generic`]). Not an Adobe/ECI profile.
pub const GENERIC_CMYK: &str = "DesignCraft Generic CMYK (SWOP-like)";
/// Profile-free CMYK (`rgb = (1−c)(1−k)` …): uncalibrated, kept for legacy numbers.
pub const DEVICE_CMYK: &str = "Device CMYK (uncalibrated)";

/// ΔE2000 above which a colour counts as out of the CMYK gamut (the gamut warning).
pub const GAMUT_THRESHOLD: f32 = 2.0;

/// ICC rendering intent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Intent {
    Perceptual,
    #[default]
    RelativeColorimetric,
    Saturation,
    AbsoluteColorimetric,
}

impl Intent {
    pub const ALL: [Intent; 4] = [Intent::Perceptual, Intent::RelativeColorimetric, Intent::Saturation, Intent::AbsoluteColorimetric];
    /// Parse `perceptual`, `relative`/`relativeColorimetric`, `saturation`, `absolute`/`absoluteColorimetric`.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().replace([' ', '_', '-'], "").as_str() {
            "perceptual" => Intent::Perceptual,
            "relative" | "relativecolorimetric" => Intent::RelativeColorimetric,
            "saturation" => Intent::Saturation,
            "absolute" | "absolutecolorimetric" => Intent::AbsoluteColorimetric,
            _ => return None,
        })
    }
    pub fn id(&self) -> &'static str {
        match self {
            Intent::Perceptual => "perceptual",
            Intent::RelativeColorimetric => "relative",
            Intent::Saturation => "saturation",
            Intent::AbsoluteColorimetric => "absolute",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Intent::Perceptual => "Perceptual",
            Intent::RelativeColorimetric => "Relative Colorimetric",
            Intent::Saturation => "Saturation",
            Intent::AbsoluteColorimetric => "Absolute Colorimetric",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProfileKind {
    Rgb,
    Cmyk,
    Gray,
}

/// A profile the user can pick.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileInfo {
    pub name: String,
    pub kind: ProfileKind,
    pub builtin: bool,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CmsError {
    #[error("unknown colour profile: {0}")]
    UnknownProfile(String),
    #[error("invalid ICC profile: {0}")]
    BadProfile(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("cannot read profile: {0}")]
    Io(String),
}

/// Edit → Color Settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorSettings {
    /// RGB working space.
    pub rgb: String,
    /// CMYK working space.
    pub cmyk: String,
    /// Conversion intent (Convert to CMYK, document colour mode).
    pub intent: Intent,
    /// Black-point compensation.
    pub bpc: bool,
}

impl Default for ColorSettings {
    fn default() -> Self {
        Self { rgb: SRGB.into(), cmyk: GENERIC_CMYK.into(), intent: Intent::RelativeColorimetric, bpc: true }
    }
}

/// Target colour model for [`Cms::convert`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    Rgb,
    Cmyk,
    Gray,
}

#[derive(Clone)]
enum CmykSpace {
    Generic(Arc<GenericCmyk>),
    Device,
    Icc(Arc<IccProfile>),
}

#[derive(Clone)]
enum RgbSpace {
    Srgb,
    Icc(Arc<IccProfile>),
}

fn generic() -> Arc<GenericCmyk> {
    static G: OnceLock<Arc<GenericCmyk>> = OnceLock::new();
    G.get_or_init(|| Arc::new(GenericCmyk::new(generic::swop_like()))).clone()
}

static USER: RwLock<Vec<Arc<IccProfile>>> = RwLock::new(Vec::new());

fn builtin_rgb_cache(name: &str) -> Option<Arc<IccProfile>> {
    static CACHE: Mutex<Vec<Arc<IccProfile>>> = Mutex::new(Vec::new());
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = c.iter().find(|p| p.name == name) {
        return Some(p.clone());
    }
    let p = Arc::new(icc::builtin_rgb(name)?);
    c.push(p.clone());
    Some(p)
}

fn user_profile(name: &str) -> Option<Arc<IccProfile>> {
    USER.read().unwrap_or_else(|e| e.into_inner()).iter().find(|p| p.name == name).cloned()
}

fn cmyk_space(name: &str) -> Result<CmykSpace, CmsError> {
    match name {
        GENERIC_CMYK => Ok(CmykSpace::Generic(generic())),
        DEVICE_CMYK => Ok(CmykSpace::Device),
        _ => match user_profile(name) {
            Some(p) if p.kind == ProfileKind::Cmyk => Ok(CmykSpace::Icc(p)),
            Some(_) => Err(CmsError::Unsupported(format!("{name} is not a CMYK profile"))),
            None => Err(CmsError::UnknownProfile(name.into())),
        },
    }
}

fn rgb_space(name: &str) -> Result<RgbSpace, CmsError> {
    if name == SRGB {
        return Ok(RgbSpace::Srgb);
    }
    if let Some(p) = builtin_rgb_cache(name) {
        return Ok(RgbSpace::Icc(p));
    }
    match user_profile(name) {
        Some(p) if p.kind == ProfileKind::Rgb => Ok(RgbSpace::Icc(p)),
        Some(_) => Err(CmsError::Unsupported(format!("{name} is not an RGB profile"))),
        None => Err(CmsError::UnknownProfile(name.into())),
    }
}

/// Every profile available (built-in first, then user-loaded).
pub fn profiles() -> Vec<ProfileInfo> {
    let mut v: Vec<ProfileInfo> = [SRGB, ADOBE_RGB, DISPLAY_P3, PROPHOTO_RGB]
        .iter()
        .map(|n| ProfileInfo { name: (*n).into(), kind: ProfileKind::Rgb, builtin: true })
        .chain([GENERIC_CMYK, DEVICE_CMYK].iter().map(|n| ProfileInfo { name: (*n).into(), kind: ProfileKind::Cmyk, builtin: true }))
        .collect();
    for p in USER.read().unwrap_or_else(|e| e.into_inner()).iter() {
        v.push(ProfileInfo { name: p.name.clone(), kind: p.kind, builtin: false });
    }
    v
}

/// Register an ICC profile from bytes (named by its description unless `name` is given).
/// Re-registering a name replaces the old profile.
pub fn register_icc(bytes: &[u8], name: Option<String>) -> Result<ProfileInfo, CmsError> {
    let p = IccProfile::from_bytes(name, bytes)?;
    if p.kind == ProfileKind::Gray {
        return Err(CmsError::Unsupported("grayscale profiles can't be working spaces yet".into()));
    }
    if [SRGB, ADOBE_RGB, DISPLAY_P3, PROPHOTO_RGB, GENERIC_CMYK, DEVICE_CMYK].contains(&p.name.as_str()) {
        return Err(CmsError::Unsupported(format!("{} clashes with a built-in profile name", p.name)));
    }
    if !p.usable() {
        return Err(CmsError::Unsupported(format!("{}: no usable transform to/from sRGB", p.name)));
    }
    let info = ProfileInfo { name: p.name.clone(), kind: p.kind, builtin: false };
    let mut u = USER.write().unwrap_or_else(|e| e.into_inner());
    u.retain(|x| x.name != info.name);
    u.push(Arc::new(p));
    drop(u);
    clear_proof_cache();
    Ok(info)
}

/// A colour-management context: resolved working spaces plus conversion settings.
#[derive(Clone)]
pub struct Cms {
    settings: ColorSettings,
    rgb: RgbSpace,
    cmyk: CmykSpace,
}

impl std::fmt::Debug for Cms {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cms").field("settings", &self.settings).finish()
    }
}

impl Default for Cms {
    fn default() -> Self {
        Self { settings: ColorSettings::default(), rgb: RgbSpace::Srgb, cmyk: CmykSpace::Generic(generic()) }
    }
}

impl Cms {
    pub fn new(settings: &ColorSettings) -> Result<Self, CmsError> {
        Ok(Self { settings: settings.clone(), rgb: rgb_space(&settings.rgb)?, cmyk: cmyk_space(&settings.cmyk)? })
    }
    pub fn settings(&self) -> &ColorSettings {
        &self.settings
    }
    pub fn rgb_is_srgb(&self) -> bool {
        matches!(self.rgb, RgbSpace::Srgb)
    }

    /// Working RGB → display sRGB.
    pub fn rgb_to_srgb(&self, rgb: [f32; 3]) -> [f32; 3] {
        match &self.rgb {
            RgbSpace::Srgb => rgb,
            RgbSpace::Icc(p) => p.to_srgb(&rgb, Intent::RelativeColorimetric).unwrap_or(rgb),
        }
    }
    /// Display sRGB → working RGB.
    pub fn srgb_to_rgb(&self, srgb: [f32; 3]) -> [f32; 3] {
        match &self.rgb {
            RgbSpace::Srgb => srgb,
            RgbSpace::Icc(p) => p.from_srgb(srgb, Intent::RelativeColorimetric).map(|v| [v[0], v[1], v[2]]).unwrap_or(srgb),
        }
    }

    fn cmyk_to_srgb_in(space: &CmykSpace, cmyk: [f32; 4], absolute: bool) -> [f32; 3] {
        let cmyk = cmyk.map(|v| v.clamp(0.0, 1.0));
        match space {
            CmykSpace::Generic(g) => g.to_srgb(cmyk, absolute),
            CmykSpace::Device => naive_cmyk_to_rgb(cmyk),
            CmykSpace::Icc(p) => {
                let i = if absolute { Intent::AbsoluteColorimetric } else { Intent::RelativeColorimetric };
                p.to_srgb(&cmyk, i).unwrap_or_else(|| naive_cmyk_to_rgb(cmyk))
            }
        }
    }

    fn srgb_to_cmyk_in(space: &CmykSpace, srgb: [f32; 3], intent: Intent, bpc: bool) -> [f32; 4] {
        match space {
            CmykSpace::Generic(g) => g.from_srgb(srgb, intent, bpc),
            CmykSpace::Device => naive_rgb_to_cmyk(srgb),
            CmykSpace::Icc(p) => p.from_srgb(srgb, intent).map(|v| [v[0], v[1], v[2], v[3]]).unwrap_or_else(|| naive_rgb_to_cmyk(srgb)),
        }
    }

    /// CMYK in the working space → display sRGB (`absolute` simulates the paper colour).
    pub fn cmyk_to_srgb(&self, cmyk: [f32; 4], absolute: bool) -> [f32; 3] {
        Self::cmyk_to_srgb_in(&self.cmyk, cmyk, absolute)
    }
    /// Display sRGB → working CMYK with `intent`.
    pub fn srgb_to_cmyk(&self, srgb: [f32; 3], intent: Intent) -> [f32; 4] {
        Self::srgb_to_cmyk_in(&self.cmyk, srgb, intent, self.settings.bpc)
    }
    /// Media-relative Lab of a working-space CMYK colour.
    pub fn cmyk_to_lab(&self, cmyk: [f32; 4]) -> Lab {
        match &self.cmyk {
            CmykSpace::Generic(g) => g.to_lab_rel(cmyk.map(|v| v.clamp(0.0, 1.0))),
            _ => lab::srgb_to_lab(self.cmyk_to_srgb(cmyk, false)),
        }
    }
    /// Lab → working CMYK with `intent`.
    pub fn lab_to_cmyk(&self, l: Lab, intent: Intent) -> [f32; 4] {
        match &self.cmyk {
            CmykSpace::Generic(g) => g.from_lab(l, intent, self.settings.bpc),
            _ => self.srgb_to_cmyk(lab::lab_to_srgb(l), intent),
        }
    }

    /// Display sRGB of any colour.
    pub fn display_rgb(&self, c: &Color) -> [f32; 3] {
        match *c {
            Color::Rgb { r, g, b } => self.rgb_to_srgb([r, g, b]),
            Color::Cmyk { c, m, y, k } => self.cmyk_to_srgb([c, m, y, k], false),
            Color::Gray { k } => [1.0 - k.clamp(0.0, 1.0); 3],
        }
    }

    /// CIE Lab (D50) of any colour.
    pub fn lab(&self, c: &Color) -> Lab {
        match *c {
            Color::Cmyk { c, m, y, k } => self.cmyk_to_lab([c, m, y, k]),
            _ => lab::srgb_to_lab(self.display_rgb(c)),
        }
    }

    /// Ink values of any colour in the working CMYK space (CMYK passes through; grey is K only).
    pub fn to_cmyk(&self, c: &Color, intent: Intent) -> [f32; 4] {
        match *c {
            Color::Cmyk { c, m, y, k } => [c, m, y, k],
            Color::Gray { k } => [0.0, 0.0, 0.0, k],
            Color::Rgb { .. } => self.srgb_to_cmyk(self.display_rgb(c), intent),
        }
    }

    /// Convert `c` into `model` (a colour already in `model` is returned unchanged).
    pub fn convert(&self, c: &Color, model: Model, intent: Intent) -> Color {
        match (c, model) {
            (Color::Rgb { .. }, Model::Rgb) | (Color::Cmyk { .. }, Model::Cmyk) | (Color::Gray { .. }, Model::Gray) => *c,
            (_, Model::Cmyk) => {
                let [c, m, y, k] = self.to_cmyk(c, intent);
                Color::Cmyk { c, m, y, k }
            }
            (_, Model::Rgb) => {
                let [r, g, b] = self.srgb_to_rgb(self.display_rgb(c));
                Color::Rgb { r, g, b }
            }
            (_, Model::Gray) => {
                // Neutral with the same L* (grey ink percentage is 1 − lightness on screen).
                let l = self.lab(c).l;
                let v = lab::xyz_to_srgb(lab::lab_to_xyz(Lab::new(l, 0.0, 0.0)))[1];
                Color::Gray { k: (1.0 - v).clamp(0.0, 1.0) }
            }
        }
    }

    /// Colour from Lab (in the working RGB space).
    pub fn from_lab(&self, l: Lab) -> Color {
        let [r, g, b] = self.srgb_to_rgb(lab::lab_to_srgb(l));
        Color::Rgb { r, g, b }
    }

    /// How far (ΔE2000) the best CMYK reproduction of `c` is from `c` (0 for CMYK/grey colours).
    pub fn gamut_error(&self, c: &Color) -> f32 {
        match c {
            Color::Rgb { .. } => {
                let srgb = self.display_rgb(c);
                let src = lab::srgb_to_lab(srgb);
                match &self.cmyk {
                    CmykSpace::Generic(g) => g.gamut_error(src),
                    CmykSpace::Device => 0.0,
                    CmykSpace::Icc(_) => {
                        let back = self.cmyk_to_srgb(self.srgb_to_cmyk(srgb, Intent::RelativeColorimetric), false);
                        delta_e2000(src, lab::srgb_to_lab(back))
                    }
                }
            }
            _ => 0.0,
        }
    }

    /// Gamut warning: can this colour be printed in the working CMYK space?
    pub fn out_of_gamut(&self, c: &Color) -> bool {
        self.gamut_error(c) > GAMUT_THRESHOLD
    }

    /// Soft-proof one display sRGB colour.
    pub fn proof_srgb(&self, srgb: [f32; 3], proof: &ProofSetup) -> [f32; 3] {
        let srgb = srgb.map(|v| v.clamp(0.0, 1.0));
        match &proof.target {
            ProofTarget::Srgb | ProofTarget::MonitorRgb => srgb,
            ProofTarget::LegacyMacRgb => srgb.map(|v| lab::linear_to_srgb(v.powf(1.8))),
            ProofTarget::Protanopia => simulate_cvd(srgb, &PROTANOPIA),
            ProofTarget::Deuteranopia => simulate_cvd(srgb, &DEUTERANOPIA),
            ProofTarget::WorkingCmyk => {
                let cmyk = self.srgb_to_cmyk(srgb, proof.intent);
                self.cmyk_to_srgb(cmyk, proof.simulate_paper)
            }
            ProofTarget::Cmyk(name) => match cmyk_space(name) {
                Ok(sp) => {
                    let cmyk = Self::srgb_to_cmyk_in(&sp, srgb, proof.intent, self.settings.bpc);
                    Self::cmyk_to_srgb_in(&sp, cmyk, proof.simulate_paper)
                }
                Err(_) => srgb,
            },
        }
    }

    /// Display sRGB of CMYK inks in the proof's CMYK space (the working space for RGB targets).
    pub fn proof_cmyk_to_srgb(&self, cmyk: [f32; 4], proof: &ProofSetup) -> [f32; 3] {
        match &proof.target {
            ProofTarget::Cmyk(name) => match cmyk_space(name) {
                Ok(sp) => Self::cmyk_to_srgb_in(&sp, cmyk, proof.simulate_paper),
                Err(_) => self.cmyk_to_srgb(cmyk, proof.simulate_paper),
            },
            _ => self.cmyk_to_srgb(cmyk, proof.simulate_paper),
        }
    }

    /// A cached 3-D lookup table implementing [`Cms::proof_srgb`] for 8-bit pixels.
    pub fn proof_lut(&self, proof: &ProofSetup) -> Arc<ProofLut> {
        let key = format!("{:?}|{:?}|{:?}|{}", self.settings, proof.target, proof.intent, proof.simulate_paper);
        let mut cache = PROOF_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, l)) = cache.iter().find(|(k, _)| *k == key) {
            return l.clone();
        }
        let lut = Arc::new(ProofLut::build(|rgb| self.proof_srgb(rgb, proof)));
        if cache.len() >= 8 {
            cache.remove(0);
        }
        cache.push((key, lut.clone()));
        lut
    }
}

static PROOF_CACHE: Mutex<Vec<(String, Arc<ProofLut>)>> = Mutex::new(Vec::new());

fn clear_proof_cache() {
    PROOF_CACHE.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

pub(crate) fn naive_cmyk_to_rgb([c, m, y, k]: [f32; 4]) -> [f32; 3] {
    [(1.0 - c) * (1.0 - k), (1.0 - m) * (1.0 - k), (1.0 - y) * (1.0 - k)]
}

pub(crate) fn naive_rgb_to_cmyk([r, g, b]: [f32; 3]) -> [f32; 4] {
    let k = 1.0 - r.max(g).max(b);
    if k >= 1.0 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    [(1.0 - r - k) / (1.0 - k), (1.0 - g - k) / (1.0 - k), (1.0 - b - k) / (1.0 - k), k]
}

// Machado, Oliveira & Fernandes (2009) dichromacy simulation matrices (severity 1.0, linear RGB).
const PROTANOPIA: [[f32; 3]; 3] = [[0.152_286, 1.052_583, -0.204_868], [0.114_503, 0.786_281, 0.099_216], [-0.003_882, -0.048_116, 1.051_998]];
const DEUTERANOPIA: [[f32; 3]; 3] = [[0.367_322, 0.860_646, -0.227_968], [0.280_085, 0.672_501, 0.047_413], [-0.011_820, 0.042_940, 0.968_881]];

fn simulate_cvd(srgb: [f32; 3], m: &[[f32; 3]; 3]) -> [f32; 3] {
    lab::mat_mul(m, lab::srgb_linear(srgb)).map(|v| lab::linear_to_srgb(v.clamp(0.0, 1.0)))
}

/// View → Proof Setup target.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "profile", rename_all = "camelCase")]
pub enum ProofTarget {
    /// Working CMYK.
    #[default]
    WorkingCmyk,
    /// A specific CMYK profile (Customize…).
    Cmyk(String),
    /// Legacy Macintosh RGB (Gamma 1.8).
    LegacyMacRgb,
    /// Internet Standard RGB (sRGB).
    Srgb,
    /// Monitor RGB (no simulation).
    MonitorRgb,
    /// Colour blindness — protanopia type.
    Protanopia,
    /// Colour blindness — deuteranopia type.
    Deuteranopia,
}

impl ProofTarget {
    pub fn parse(s: &str) -> Option<Self> {
        if let Some(p) = s.strip_prefix("cmyk:") {
            return Some(ProofTarget::Cmyk(p.to_string()));
        }
        Some(match s {
            "workingCmyk" | "cmyk" => ProofTarget::WorkingCmyk,
            "legacyMacRgb" | "macRgb" => ProofTarget::LegacyMacRgb,
            "srgb" | "sRGB" => ProofTarget::Srgb,
            "monitorRgb" | "monitor" => ProofTarget::MonitorRgb,
            "protanopia" => ProofTarget::Protanopia,
            "deuteranopia" => ProofTarget::Deuteranopia,
            _ => return None,
        })
    }
    pub fn id(&self) -> String {
        match self {
            ProofTarget::WorkingCmyk => "workingCmyk".into(),
            ProofTarget::Cmyk(p) => format!("cmyk:{p}"),
            ProofTarget::LegacyMacRgb => "legacyMacRgb".into(),
            ProofTarget::Srgb => "srgb".into(),
            ProofTarget::MonitorRgb => "monitorRgb".into(),
            ProofTarget::Protanopia => "protanopia".into(),
            ProofTarget::Deuteranopia => "deuteranopia".into(),
        }
    }
    /// Whether the target simulates a CMYK press (separations make sense).
    pub fn is_cmyk(&self) -> bool {
        matches!(self, ProofTarget::WorkingCmyk | ProofTarget::Cmyk(_))
    }
}

/// View → Proof Setup / Proof Colors / Separations Preview state for rendering.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofSetup {
    pub target: ProofTarget,
    pub intent: Intent,
    /// Simulate Paper Color (absolute colorimetric display of the press).
    #[serde(default)]
    pub simulate_paper: bool,
    /// Separations Preview: names of the visible plates ("Cyan", "Magenta", "Yellow", "Black",
    /// spot swatch names). `None` = normal composite proof.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub separations: Option<Vec<String>>,
}

/// Process plate names in order.
pub const PROCESS_PLATES: [&str; 4] = ["Cyan", "Magenta", "Yellow", "Black"];

const LUT_N: usize = 17;

/// A 17³ RGB → RGB lookup table with trilinear interpolation.
pub struct ProofLut {
    data: Vec<[f32; 3]>,
}

impl ProofLut {
    pub fn build(f: impl Fn([f32; 3]) -> [f32; 3]) -> Self {
        let n = LUT_N;
        let mut data = Vec::with_capacity(n * n * n);
        for r in 0..n {
            for g in 0..n {
                for b in 0..n {
                    let s = (n - 1) as f32;
                    data.push(f([r as f32 / s, g as f32 / s, b as f32 / s]));
                }
            }
        }
        Self { data }
    }
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        let n = LUT_N;
        let s = (n - 1) as f32;
        let p = rgb.map(|v| v.clamp(0.0, 1.0) * s);
        let i = p.map(|v| (v.floor() as usize).min(n - 2));
        let t = [p[0] - i[0] as f32, p[1] - i[1] as f32, p[2] - i[2] as f32];
        let at = |r: usize, g: usize, b: usize| self.data[(r * n + g) * n + b];
        let mut out = [0.0f32; 3];
        for (dr, wr) in [(0, 1.0 - t[0]), (1, t[0])] {
            for (dg, wg) in [(0, 1.0 - t[1]), (1, t[1])] {
                for (db, wb) in [(0, 1.0 - t[2]), (1, t[2])] {
                    let w = wr * wg * wb;
                    if w == 0.0 {
                        continue;
                    }
                    let v = at(i[0] + dr, i[1] + dg, i[2] + db);
                    for c in 0..3 {
                        out[c] += w * v[c];
                    }
                }
            }
        }
        out
    }
    pub fn apply8(&self, rgb: [u8; 3]) -> [u8; 3] {
        self.apply(rgb.map(|v| v as f32 / 255.0)).map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
    }
}

// ---------- the active (process-wide) settings ----------

static ACTIVE: RwLock<Option<Arc<Cms>>> = RwLock::new(None);
static RGB_IS_SRGB: AtomicBool = AtomicBool::new(true);
static CMYK_IS_DEVICE: AtomicBool = AtomicBool::new(false);

/// The active colour-management context (Edit → Color Settings).
pub fn active() -> Arc<Cms> {
    if let Some(c) = ACTIVE.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        return c.clone();
    }
    let mut w = ACTIVE.write().unwrap_or_else(|e| e.into_inner());
    w.get_or_insert_with(|| Arc::new(Cms::default())).clone()
}

pub fn active_settings() -> ColorSettings {
    active().settings.clone()
}

/// Replace the active settings (fails, leaving them unchanged, if a profile is unknown).
pub fn set_active(settings: &ColorSettings) -> Result<(), CmsError> {
    let cms = Cms::new(settings)?;
    RGB_IS_SRGB.store(cms.rgb_is_srgb(), Ordering::Relaxed);
    CMYK_IS_DEVICE.store(matches!(cms.cmyk, CmykSpace::Device), Ordering::Relaxed);
    *ACTIVE.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(cms));
    Ok(())
}

/// Fast path for [`Color::to_rgb`]: RGB colours need no conversion while the working space is sRGB.
pub(crate) fn rgb_is_srgb() -> bool {
    RGB_IS_SRGB.load(Ordering::Relaxed)
}
pub(crate) fn cmyk_is_device() -> bool {
    CMYK_IS_DEVICE.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests;
