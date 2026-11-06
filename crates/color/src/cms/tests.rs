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
fn srgb_profile_matches_analytic() {
    // The built-in sRGB ICC profile agrees with our analytic sRGB maths.
    let p = icc::builtin_rgb(SRGB).unwrap();
    for rgb in [[0.2, 0.5, 0.8], [1.0, 0.0, 0.0], [0.5, 0.5, 0.5]] {
        let out = p.to_srgb(&rgb, Intent::RelativeColorimetric, false).unwrap();
        assert!(de(out, rgb) < 0.5, "{rgb:?} → {out:?}");
    }
}

/// sRGB 0..255 of CMYK percentages.
fn shown(c: &Cms, [cy, m, y, k]: [f32; 4]) -> [f32; 3] {
    c.display_rgb(&Color::cmyk(cy / 100.0, m / 100.0, y / 100.0, k / 100.0)).map(|v| v * 255.0)
}

/// Swatches in the default CMYK space (Coated FOGRA39), shown relative colorimetric with black
/// point compensation, and the sRGB reference values they show as (u8, within 2).
const FOGRA39_ANCHORS: [([f32; 4], [f32; 3]); 4] = [
    ([0.0, 50.0, 100.0, 0.0], [243.0, 146.0, 0.0]),
    ([0.0, 60.0, 100.0, 0.0], [239.0, 123.0, 0.0]),
    ([0.0, 0.0, 0.0, 100.0], [28.0, 28.0, 27.0]),
    ([100.0, 100.0, 0.0, 0.0], [48.0, 38.0, 131.0]),
];

#[test]
fn default_cmyk_swatches_show_as_coated_fogra39() {
    assert_eq!(ColorSettings::default().cmyk, COATED_FOGRA39);
    for c in [Cms::default(), cms()] {
        for (cmyk, want) in FOGRA39_ANCHORS {
            let got = shown(&c, cmyk);
            assert!(got.iter().zip(want).all(|(g, w)| (g - w).abs() <= 2.0), "{cmyk:?}: {got:?} vs {want:?}");
        }
    }
}

#[test]
fn saved_cmyk_names_still_load() {
    for name in [GENERIC_CMYK, DEVICE_CMYK, COATED_FOGRA39] {
        let s = ColorSettings { cmyk: name.into(), ..Default::default() };
        assert!(Cms::new(&s).is_ok(), "{name}");
        assert_eq!(s.with_known_profiles(), s);
    }
    // The names of the profiles the built-in stands in for show the same colours.
    let stand_in = Cms::new(&ColorSettings { cmyk: "Coated FOGRA39 (ISO 12647-2:2004)".into(), ..Default::default() }).unwrap();
    assert_eq!(shown(&stand_in, [0.0, 50.0, 100.0, 0.0]), shown(&cms(), [0.0, 50.0, 100.0, 0.0]));
    // A related press condition with a different ink limit has no stand-in.
    let iso300 = ColorSettings { cmyk: "ISO Coated v2 300% (ECI)".into(), ..Default::default() };
    assert!(matches!(Cms::new(&iso300), Err(CmsError::UnknownProfile(_))));
    // A name that isn't available falls back to the default, keeping the other settings.
    let saved = ColorSettings { rgb: ADOBE_RGB.into(), cmyk: "Some Press (gone)".into(), bpc: false, ..Default::default() };
    assert!(Cms::new(&saved).is_err());
    let loaded = saved.with_known_profiles();
    assert_eq!(loaded, ColorSettings { rgb: ADOBE_RGB.into(), cmyk: COATED_FOGRA39.into(), bpc: false, ..Default::default() });
    assert!(Cms::new(&loaded).is_ok());
}

#[test]
fn output_intent_follows_the_working_cmyk_space() {
    let fogra = cms().output_intent().unwrap();
    assert_eq!((fogra.identifier, fogra.condition.as_str()), ("FOGRA39", COATED_FOGRA39));
    assert_eq!(&*fogra.icc, craft_cms::builtin::COATED_FOGRA39_ICC);
    for name in [GENERIC_CMYK, DEVICE_CMYK] {
        let g = Cms::new(&ColorSettings { cmyk: name.into(), ..Default::default() }).unwrap().output_intent().unwrap();
        assert_eq!((g.identifier, g.condition.as_str()), ("Custom", GENERIC_CMYK), "{name}");
        assert_eq!(&*g.icc, icc_out::generic_cmyk_icc().unwrap());
    }
}

#[test]
fn paper_white_and_rich_black() {
    let c = cms();
    assert_eq!(c.srgb_to_cmyk([1.0, 1.0, 1.0], Intent::RelativeColorimetric), [0.0; 4]);
    for intent in [Intent::Perceptual, Intent::RelativeColorimetric, Intent::Saturation] {
        assert_eq!(c.lab_to_cmyk(lab::Lab::new(100.0, 0.0, 0.0), intent), [0.0; 4], "{intent:?}");
    }
    let k = c.srgb_to_cmyk([0.0, 0.0, 0.0], Intent::RelativeColorimetric);
    assert!(k[3] > 0.8, "black generation puts most of the black in K: {k:?}");
    assert!(k.iter().sum::<f32>() <= 3.3 + 1e-3, "Coated FOGRA39 total ink limit (330 %): {k:?}");
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
    // FOGRA39 paper is slightly blue (b* -2); Generic CMYK's slightly warm.
    assert!(abs.iter().all(|v| *v < 0.97) && abs[2] > abs[0], "paper is darker and slightly blue: {abs:?}");
    let g = Cms::new(&ColorSettings { cmyk: GENERIC_CMYK.into(), ..Default::default() }).unwrap();
    let abs = g.cmyk_to_srgb([0.0; 4], true);
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
