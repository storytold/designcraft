//! ICC profiles through `craft-cms` (pure Rust): built-in RGB and CMYK spaces and user-supplied
//! `.icc` files.

use std::sync::{Arc, OnceLock};

use craft_cms::{Builtin, ColorSpace, Profile, Transform, TransformOptions};

use super::lab::Lab;
use super::{CmsError, Intent, ProfileKind};

type Xf = Option<Arc<Transform>>;

/// One transform per intent (in [`Intent::ALL`] order), without and with black point
/// compensation.
type Slots = [[OnceLock<Xf>; 2]; 4];

fn slot(slots: &Slots, intent: Intent, bpc: bool) -> &OnceLock<Xf> {
    let by_intent = match intent {
        Intent::Perceptual => &slots[0],
        Intent::RelativeColorimetric => &slots[1],
        Intent::Saturation => &slots[2],
        Intent::AbsoluteColorimetric => &slots[3],
    };
    if bpc { &by_intent[1] } else { &by_intent[0] }
}

/// A parsed ICC profile plus lazily built transforms to and from sRGB and Lab.
pub struct IccProfile {
    pub name: String,
    pub kind: ProfileKind,
    profile: Arc<Profile>,
    to_srgb: Slots,
    from_srgb: Slots,
    to_lab: OnceLock<Xf>,
    from_lab: Slots,
}

impl std::fmt::Debug for IccProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IccProfile").field("name", &self.name).field("kind", &self.kind).finish()
    }
}

fn build(src: &Profile, dst: &Profile, intent: Intent, bpc: bool) -> Xf {
    Transform::with_options(src, dst, TransformOptions { intent: intent.into(), bpc, precise_float: true }).ok().map(Arc::new)
}

/// Evaluate one colour exactly, each output clamped to 0..1; `None` when the transform's channel
/// counts don't fit or the result isn't finite.
fn eval(x: &Transform, src: &[f32], outputs: usize) -> Option<Vec<f32>> {
    if x.inputs() != src.len() || x.outputs() != outputs {
        return None;
    }
    let src: Vec<f32> = src.iter().map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }).collect();
    let mut out = vec![0.0f32; outputs];
    x.eval(&src, &mut out).ok()?;
    out.iter().all(|o| o.is_finite()).then(|| out.into_iter().map(|v| v.clamp(0.0, 1.0)).collect())
}

fn eval3(x: &Transform, src: &[f32]) -> Option<[f32; 3]> {
    match eval(x, src, 3)?.as_slice() {
        &[a, b, c] => Some([a, b, c]),
        _ => None,
    }
}

impl IccProfile {
    pub fn from_profile(name: Option<String>, profile: Arc<Profile>) -> Result<Self, CmsError> {
        let kind = match profile.color_space {
            ColorSpace::Rgb => ProfileKind::Rgb,
            ColorSpace::Cmyk => ProfileKind::Cmyk,
            ColorSpace::Gray => ProfileKind::Gray,
            other => return Err(CmsError::Unsupported(format!("profile colour space {other:?} is not RGB, CMYK or Gray"))),
        };
        let desc = profile.description.trim_matches(char::from(0)).trim();
        let name = name.or_else(|| (!desc.is_empty()).then(|| desc.to_string())).unwrap_or_else(|| format!("Untitled {kind:?} profile"));
        Ok(Self {
            name,
            kind,
            profile,
            to_srgb: Default::default(),
            from_srgb: Default::default(),
            to_lab: OnceLock::new(),
            from_lab: Default::default(),
        })
    }

    pub fn from_bytes(name: Option<String>, bytes: &[u8]) -> Result<Self, CmsError> {
        let p = Profile::parse(bytes).map_err(|e| CmsError::BadProfile(e.to_string()))?;
        Self::from_profile(name, Arc::new(p))
    }

    /// The parsed profile.
    pub fn profile(&self) -> &Arc<Profile> {
        &self.profile
    }

    fn channels(&self) -> usize {
        match self.kind {
            ProfileKind::Cmyk => 4,
            ProfileKind::Gray => 1,
            ProfileKind::Rgb => 3,
        }
    }

    fn to_xf(&self, intent: Intent, bpc: bool) -> Xf {
        slot(&self.to_srgb, intent, bpc).get_or_init(|| build(&self.profile, Builtin::Srgb.profile(), intent, bpc)).clone()
    }

    fn inverse_xf(&self, intent: Intent, bpc: bool) -> Xf {
        slot(&self.from_srgb, intent, bpc).get_or_init(|| build(Builtin::Srgb.profile(), &self.profile, intent, bpc)).clone()
    }

    /// Device values (RGB / CMYK / Gray, 0..1; missing channels are 0) → sRGB.
    pub fn to_srgb(&self, v: &[f32], intent: Intent, bpc: bool) -> Option<[f32; 3]> {
        let mut src = v.to_vec();
        src.resize(self.channels(), 0.0);
        eval3(&*self.to_xf(intent, bpc)?, &src)
    }

    /// sRGB → device values.
    pub fn from_srgb(&self, rgb: [f32; 3], intent: Intent, bpc: bool) -> Option<Vec<f32>> {
        let x = self.inverse_xf(intent, bpc)?;
        eval(&x, &rgb, self.channels())
    }

    /// Media-relative Lab (D50) of device values: relative colorimetric, no black point
    /// compensation.
    pub fn to_lab(&self, v: &[f32]) -> Option<Lab> {
        let x = self.to_lab.get_or_init(|| build(&self.profile, Builtin::LabD50.profile(), Intent::RelativeColorimetric, false)).clone()?;
        let mut src = v.to_vec();
        src.resize(self.channels(), 0.0);
        let [l, a, b] = eval3(&x, &src)?;
        // Lab D50 device values use the ICC v4 encoding: L/100, (a + 128)/255.
        Some(Lab::new(l * 100.0, a * 255.0 - 128.0, b * 255.0 - 128.0))
    }

    /// Lab (D50) → device values with `intent`.
    pub fn from_lab(&self, lab: Lab, intent: Intent, bpc: bool) -> Option<Vec<f32>> {
        let x = slot(&self.from_lab, intent, bpc).get_or_init(|| build(Builtin::LabD50.profile(), &self.profile, intent, bpc)).clone()?;
        eval(&x, &[lab.l / 100.0, (lab.a + 128.0) / 255.0, (lab.b + 128.0) / 255.0], self.channels())
    }

    /// Whether transforms can be built for this profile (checked at registration).
    pub fn usable(&self) -> bool {
        self.to_xf(Intent::RelativeColorimetric, false).is_some() && self.inverse_xf(Intent::RelativeColorimetric, false).is_some()
    }
}

fn builtin(name: &str, b: Builtin) -> Option<IccProfile> {
    IccProfile::from_profile(Some(name.to_string()), Arc::new(b.profile().clone())).ok()
}

/// Built-in RGB working spaces from craft-cms's built-in profiles.
pub fn builtin_rgb(name: &str) -> Option<IccProfile> {
    let b = match name {
        super::ADOBE_RGB => Builtin::AdobeRgbCompat,
        super::DISPLAY_P3 => Builtin::DisplayP3,
        super::PROPHOTO_RGB => Builtin::ProPhotoCompat,
        super::SRGB => Builtin::Srgb,
        _ => return None,
    };
    builtin(name, b)
}

/// The Coated FOGRA39 built-in, listed as [`super::COATED_FOGRA39`].
pub fn coated_fogra39() -> Option<IccProfile> {
    builtin(super::COATED_FOGRA39, Builtin::CoatedFogra39)
}
