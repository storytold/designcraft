//! An ICC output profile of our Generic CMYK press model (for PDF/X output intents): lut16
//! tables sampled from the model — CMYK → Lab from the forward model, Lab → CMYK from its
//! inverse (separation with black generation and the ink limit). The profile is ours, built in
//! code; it describes our model, not a measured press.

use std::sync::OnceLock;

use craft_cms::profile::{Lut, LutKind};
use craft_cms::{Builtin, Clut, Curve, Intent as CmsIntent, Profile, pipeline::Stage};

use super::lab::Lab;
use super::{CmsError, Intent};

const A2B_GRID: usize = 9;
const B2A_GRID: usize = 17;

/// ICC v2 legacy 16-bit Lab encoding (lut16Type), normalised to 0..1.
const V2_L: f32 = 100.0 * 65535.0 / 65280.0;
const V2_AB: f32 = 65535.0 / 256.0;

fn enc_lab(l: Lab) -> [f32; 3] {
    [l.l / V2_L, (l.a + 128.0) / V2_AB, (l.b + 128.0) / V2_AB].map(|v| v.clamp(0.0, 1.0))
}

fn dec_lab(v: &[f32]) -> Lab {
    let at = |i: usize| v.get(i).copied().unwrap_or(0.0);
    Lab::new(at(0) * V2_L, at(1) * V2_AB - 128.0, at(2) * V2_AB - 128.0)
}

fn lut16(clut: Clut) -> Lut {
    let (i, o) = (clut.inputs(), clut.outputs());
    let ident = |n: usize| Stage::Curves(vec![Curve::Table(vec![0.0, 1.0]); n]);
    Lut { kind: LutKind::Lut16, inputs: i, outputs: o, stages: vec![ident(i), Stage::Clut(clut), ident(o)] }
}

/// The Generic CMYK model as an ICC output profile (built once).
pub fn generic_cmyk_profile() -> Result<&'static Profile, CmsError> {
    static P: OnceLock<Result<Profile, CmsError>> = OnceLock::new();
    P.get_or_init(build_generic_cmyk).as_ref().map_err(Clone::clone)
}

fn build_generic_cmyk() -> Result<Profile, CmsError> {
    let bad = |e: craft_cms::CmsError| CmsError::BadProfile(format!("{}: {e}", super::GENERIC_CMYK));
    let g = super::generic();
    // CMYK → Lab (media-relative).
    let a2b = Clut::sample(vec![A2B_GRID; 4], 3, |inp, out| {
        let at = |i: usize| inp.get(i).copied().unwrap_or(0.0);
        out.copy_from_slice(&enc_lab(g.to_lab_rel([at(0), at(1), at(2), at(3)])));
    })
    .map_err(bad)?;
    // Lab → CMYK through the model's separation.
    let b2a = Clut::sample(vec![B2A_GRID; 3], 4, |inp, out| {
        let cmyk = g.from_lab(dec_lab(inp), Intent::RelativeColorimetric, true);
        out.copy_from_slice(&cmyk.map(|v| v.clamp(0.0, 1.0)));
    })
    .map_err(bad)?;
    let (a2b, b2a) = (lut16(a2b), lut16(b2a));
    // craft-cms profiles can't be constructed field by field outside the crate, so the
    // tables replace those of a built-in CMYK output profile and the result is re-encoded.
    let mut p = Builtin::CoatedCmyk.profile().clone();
    p.rendering_intent = CmsIntent::RelativeColorimetric;
    p.description = super::GENERIC_CMYK.into();
    p.copyright = "No copyright; made by DesignCraft from its parametric press model. Use freely.".into();
    p.white_point = [0.9642, 1.0, 0.8249];
    p.black_point = None;
    p.chad = None;
    p.a2b = [Some(a2b.clone()), Some(a2b.clone()), Some(a2b)];
    p.b2a = [Some(b2a.clone()), Some(b2a.clone()), Some(b2a)];
    Ok(p.with_encoded_bytes())
}

/// [`generic_cmyk_profile`]'s ICC bytes.
pub fn generic_cmyk_icc() -> Result<&'static [u8], CmsError> {
    static B: OnceLock<Result<std::sync::Arc<Vec<u8>>, CmsError>> = OnceLock::new();
    B.get_or_init(|| generic_cmyk_profile().map(Profile::to_bytes)).as_ref().map(|b| b.as_slice()).map_err(Clone::clone)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_cmyk_profile_round_trips() {
        let bytes = generic_cmyk_icc().unwrap();
        assert!(bytes.len() > 1000);
        assert_eq!(&bytes[36..40], b"acsp");
        assert_eq!(&bytes[16..20], b"CMYK");
        assert_eq!(&bytes[12..16], b"prtr");
        // Read back: CMYK → sRGB through the profile is close to the model.
        let p = super::super::icc::IccProfile::from_bytes(None, bytes).expect("parses");
        assert_eq!(p.name, super::super::GENERIC_CMYK);
        let g = super::super::generic();
        for cmyk in [[0.0, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 1.0, 0.0], [0.2, 0.3, 0.1, 0.5]] {
            let via = p.to_srgb(&cmyk, Intent::RelativeColorimetric, false).expect("converts");
            let model = g.to_srgb(cmyk, false);
            for i in 0..3 {
                assert!((via[i] - model[i]).abs() < 0.04, "{cmyk:?}: {via:?} vs {model:?}");
            }
        }
    }
}
