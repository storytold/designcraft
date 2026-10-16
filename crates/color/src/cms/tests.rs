use super::*;

fn cms() -> Cms {
    Cms::new(&ColorSettings::default()).unwrap()
}

/// Without black-point compensation, relative colorimetric reproduces in-gamut colours exactly.
fn cms_no_bpc() -> Cms {
    Cms::new(&ColorSettings { bpc: false, ..Default::default() }).unwrap()
}

fn de(a: [f32; 3], b: [f32; 3]) -> f32 {
    delta_e2000(lab::srgb_to_lab(a), lab::srgb_to_lab(b))
}

#[test]
fn intents_parse_and_name() {
    for i in Intent::ALL {
        assert_eq!(Intent::parse(i.id()), Some(i));
        assert_eq!(Intent::parse(i.label()), Some(i));
    }
    assert_eq!(Intent::parse("bogus"), None);
}

#[test]
fn srgb_via_moxcms_matches_analytic() {
    // The built-in sRGB ICC profile through moxcms agrees with our analytic sRGB maths.
    let p = icc::builtin_rgb(SRGB).unwrap();
    for rgb in [[0.2, 0.5, 0.8], [1.0, 0.0, 0.0], [0.5, 0.5, 0.5]] {
        let out = p.to_srgb(&rgb, Intent::RelativeColorimetric).unwrap();
        assert!(de(out, rgb) < 0.5, "{rgb:?} → {out:?}");
    }
}

#[test]
fn paper_white_and_rich_black() {
    let c = cms();
    assert_eq!(c.srgb_to_cmyk([1.0, 1.0, 1.0], Intent::RelativeColorimetric), [0.0; 4]);
    let k = c.srgb_to_cmyk([0.0, 0.0, 0.0], Intent::RelativeColorimetric);
    assert!(k[3] > 0.8, "black generation puts most of the black in K: {k:?}");
    assert!(k.iter().sum::<f32>() <= 3.0 + 1e-3, "TAC limit: {k:?}");
    assert!(k[0] > 0.3 && k[1] > 0.3 && k[2] > 0.3, "rich black: {k:?}");
    assert_eq!(c.cmyk_to_srgb([0.0; 4], false), [1.0, 1.0, 1.0]);
}

#[test]
fn rgb_cmyk_rgb_roundtrip_in_gamut() {
    let c = cms_no_bpc();
    for rgb in [[0.8, 0.6, 0.5], [0.5, 0.5, 0.5], [0.3, 0.45, 0.35], [0.9, 0.85, 0.3], [0.55, 0.4, 0.6], [0.35, 0.55, 0.7], [0.75, 0.75, 0.75]] {
        let cmyk = c.srgb_to_cmyk(rgb, Intent::RelativeColorimetric);
        let back = c.cmyk_to_srgb(cmyk, false);
        assert!(de(rgb, back) < 1.5, "{rgb:?} → {cmyk:?} → {back:?} ΔE {}", de(rgb, back));
    }
}

#[test]
fn cmyk_lab_cmyk_roundtrip() {
    let c = cms_no_bpc();
    for cmyk in [[0.2, 0.4, 0.6, 0.1], [0.0, 0.0, 0.0, 0.5], [0.5, 0.1, 0.0, 0.0], [0.1, 0.7, 0.3, 0.2]] {
        let l = c.cmyk_to_lab(cmyk);
        // Lab here is media-relative, i.e. what relative colorimetric reproduces exactly.
        let back = c.lab_to_cmyk(l, Intent::RelativeColorimetric);
        let l2 = c.cmyk_to_lab(back);
        assert!(delta_e2000(l, l2) < 1.0, "{cmyk:?} → {l:?} → {back:?} → {l2:?}");
    }
}

#[test]
fn gcr_puts_grey_into_black() {
    let k = cms().srgb_to_cmyk([0.4, 0.4, 0.4], Intent::RelativeColorimetric);
    assert!(k[3] > 0.2, "{k:?}");
    let spread = k[0].max(k[1]).max(k[2]) - k[0].min(k[1]).min(k[2]);
    assert!(spread < 0.2, "neutral stays balanced: {k:?}");
}

#[test]
fn gamut_warning() {
    let c = cms();
    assert!(c.out_of_gamut(&Color::rgb(0.0, 0.0, 1.0)), "sRGB blue is outside SWOP-like CMYK");
    assert!(c.out_of_gamut(&Color::rgb(0.0, 1.0, 0.0)), "sRGB green is outside");
    assert!(!c.out_of_gamut(&Color::rgb(0.8, 0.6, 0.5)), "skin tone is printable");
    assert!(!c.out_of_gamut(&Color::rgb(0.5, 0.5, 0.5)));
    assert!(!c.out_of_gamut(&Color::cmyk(1.0, 0.0, 0.0, 0.0)), "CMYK colours are in gamut by definition");
    // The uncalibrated device space has no gamut limits.
    let dev = Cms::new(&ColorSettings { cmyk: DEVICE_CMYK.into(), ..Default::default() }).unwrap();
    assert!(!dev.out_of_gamut(&Color::rgb(0.0, 0.0, 1.0)));
}

#[test]
fn intents_differ() {
    let c = cms();
    let blue = [0.0, 0.0, 1.0];
    let res: Vec<[f32; 4]> = Intent::ALL.iter().map(|i| c.srgb_to_cmyk(blue, *i)).collect();
    let dist = |a: [f32; 4], b: [f32; 4]| (0..4).map(|i| (a[i] - b[i]).abs()).sum::<f32>();
    assert!(dist(res[0], res[1]) > 0.02, "perceptual vs relative: {res:?}");
    assert!(dist(res[2], res[1]) > 0.02, "saturation vs relative: {res:?}");
    // Absolute colorimetry keeps the source white brighter than paper → less ink on light greys.
    let g = [0.88, 0.88, 0.88];
    let rel = c.srgb_to_cmyk(g, Intent::RelativeColorimetric);
    let abs = c.srgb_to_cmyk(g, Intent::AbsoluteColorimetric);
    assert!(rel.iter().sum::<f32>() > abs.iter().sum::<f32>() + 0.02, "{rel:?} vs {abs:?}");
    // Perceptual compresses in-gamut saturated colours too (relative leaves them alone).
    let red = lab::srgb_to_lab([0.8, 0.25, 0.25]);
    let pr = c.cmyk_to_lab(c.srgb_to_cmyk([0.8, 0.25, 0.25], Intent::Perceptual));
    let rr = c.cmyk_to_lab(c.srgb_to_cmyk([0.8, 0.25, 0.25], Intent::RelativeColorimetric));
    assert!(delta_e2000(rr, red) < delta_e2000(pr, red), "relative is closer for in-gamut colours");
}

#[test]
fn simulate_paper_tints_white() {
    let c = cms();
    let rel = c.cmyk_to_srgb([0.0; 4], false);
    let abs = c.cmyk_to_srgb([0.0; 4], true);
    assert_eq!(rel, [1.0; 3]);
    assert!(abs[0] < 0.98 && abs[2] < abs[0], "paper is darker and slightly warm: {abs:?}");
}

#[test]
fn device_cmyk_keeps_legacy_numbers() {
    let dev = Cms::new(&ColorSettings { cmyk: DEVICE_CMYK.into(), ..Default::default() }).unwrap();
    assert_eq!(dev.cmyk_to_srgb([1.0, 0.0, 0.0, 0.0], false), [0.0, 1.0, 1.0]);
    assert_eq!(dev.srgb_to_cmyk([1.0, 0.0, 0.0], Intent::Perceptual), [0.0, 1.0, 1.0, 0.0]);
}

#[test]
fn convert_between_models() {
    let c = cms();
    let red = Color::rgb(0.9, 0.2, 0.2);
    let cmyk = c.convert(&red, Model::Cmyk, Intent::RelativeColorimetric);
    assert!(matches!(cmyk, Color::Cmyk { .. }));
    assert_eq!(c.convert(&cmyk, Model::Cmyk, Intent::Perceptual), cmyk, "idempotent");
    let back = c.convert(&cmyk, Model::Rgb, Intent::RelativeColorimetric);
    assert!(de(back.to_rgb_uncalibrated(), [0.9, 0.2, 0.2]) < 6.0, "{back:?}");
    let g = c.convert(&Color::rgb(0.5, 0.5, 0.5), Model::Gray, Intent::RelativeColorimetric);
    let Color::Gray { k } = g else { panic!() };
    assert!((k - 0.5).abs() < 0.01, "{k}");
}

#[test]
fn wide_gamut_working_space() {
    let c = Cms::new(&ColorSettings { rgb: ADOBE_RGB.into(), ..Default::default() }).unwrap();
    assert!(!c.rgb_is_srgb());
    let grey = c.rgb_to_srgb([0.5, 0.5, 0.5]);
    assert!(de(grey, [0.5, 0.5, 0.5]) < 1.5, "{grey:?}");
    // Adobe RGB green is outside sRGB: it clips to (nearly) full sRGB green.
    let g = c.rgb_to_srgb([0.0, 1.0, 0.0]);
    assert!(g[1] > 0.95 && g[0] < 0.1, "{g:?}");
    let back = c.srgb_to_rgb(c.rgb_to_srgb([0.4, 0.5, 0.3]));
    assert!(de(back, [0.4, 0.5, 0.3]) < 1.0);
    assert!(matches!(Cms::new(&ColorSettings { rgb: "Nope".into(), ..Default::default() }), Err(CmsError::UnknownProfile(_))));
}

#[test]
fn proof_lut_matches_direct_proof() {
    let c = cms();
    let p = ProofSetup::default();
    let lut = c.proof_lut(&p);
    for rgb in [[0.8, 0.6, 0.5], [0.0, 0.0, 1.0], [0.3, 0.9, 0.2]] {
        let direct = c.proof_srgb(rgb, &p);
        let via_lut = lut.apply(rgb);
        assert!(de(direct, via_lut) < 2.5, "{rgb:?}: {direct:?} vs {via_lut:?}");
    }
    // Out-of-gamut blue gets visibly duller in the proof.
    let b = c.proof_srgb([0.0, 0.0, 1.0], &p);
    assert!(de(b, [0.0, 0.0, 1.0]) > 5.0, "{b:?}");
}

#[test]
fn colour_blindness_proofs() {
    let c = cms();
    let p = ProofSetup { target: ProofTarget::Protanopia, ..Default::default() };
    let red = c.proof_srgb([0.9, 0.1, 0.1], &p);
    let green = c.proof_srgb([0.1, 0.6, 0.1], &p);
    assert!(de(red, green) < de([0.9, 0.1, 0.1], [0.1, 0.6, 0.1]) * 0.5, "red/green confusion");
    let d = ProofSetup { target: ProofTarget::Deuteranopia, ..Default::default() };
    assert_ne!(c.proof_srgb([0.9, 0.1, 0.1], &d), [0.9, 0.1, 0.1]);
    let m = ProofSetup { target: ProofTarget::MonitorRgb, ..Default::default() };
    assert_eq!(c.proof_srgb([0.2, 0.3, 0.4], &m), [0.2, 0.3, 0.4]);
    assert_eq!(ProofTarget::parse(&ProofTarget::Cmyk("X".into()).id()), Some(ProofTarget::Cmyk("X".into())));
}

#[test]
fn bad_icc_rejected() {
    assert!(matches!(register_icc(b"not a profile", None), Err(CmsError::BadProfile(_))));
}

#[cfg(target_os = "macos")]
#[test]
fn loads_system_cmyk_icc() {
    let path = std::path::Path::new("/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc");
    let Ok(bytes) = std::fs::read(path) else { return };
    let info = register_icc(&bytes, None).unwrap();
    assert_eq!(info.kind, ProfileKind::Cmyk);
    assert!(profiles().iter().any(|p| p.name == info.name && !p.builtin));
    let c = Cms::new(&ColorSettings { cmyk: info.name.clone(), ..Default::default() }).unwrap();
    let rgb = [0.7, 0.5, 0.4];
    let back = c.cmyk_to_srgb(c.srgb_to_cmyk(rgb, Intent::RelativeColorimetric), false);
    assert!(de(rgb, back) < 5.0, "{back:?}");
    assert_eq!(c.cmyk_to_srgb([0.0; 4], false).map(|v| (v * 255.0).round() as u8), [255, 255, 255]);
}
