//! Synthetic `.af` version-12 text regression oracles; see support/text_fixture.rs for provenance.
//! Unicode ranges below test the scalar-index contract. Broad complex typography still needs
//! independent visual validation.
#![cfg(feature = "synth")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support {
    pub mod text_fixture;
}
use designcraft_affinity::{
    Document, Limits,
    model::{Align, Kind, Text},
    paint::{Color, Paint},
    synth::{F, tag},
};
use support::text_fixture::*;

fn read(blocks: Vec<F>) -> Document {
    designcraft_affinity::read(&document(blocks, false, 96.0, IDENTITY), Limits::default()).unwrap()
}
fn text(d: &Document) -> &Text {
    match &d.spreads[0].nodes[0].kind {
        Kind::Text(t) => t,
        other => panic!("expected text, got {other:?}"),
    }
}
fn content(t: &Text) -> String {
    t.runs.iter().map(|r| r.text.as_str()).collect()
}
fn warning(d: &Document, needle: &str) -> bool {
    d.warnings.iter().any(|w| w.contains(needle))
}

#[test]
fn mixed_fonts_keep_unicode_and_attribute_boundaries() {
    let d = read(vec![block(
        utf8("Aé😀e\u{301}אב日本\u{2028}x\u{2029}\0"),
        vec![
            run(3, Some(attrs("Family One", "FamilyOne-Regular", 400, false, 16.0))),
            run(9, Some(attrs("Family Two", "FamilyTwo-BoldItalic", 700, true, 24.0))),
            run(13, Some(attrs("Family Three", "FamilyThree-Light", 300, false, 20.0))),
        ],
        &[1],
    )]);
    let t = text(&d);
    assert_eq!(t.runs.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), ["Aé😀", "e\u{301}אב日本", "\u{2028}x\u{2029}"]);
    assert_eq!(content(t), "Aé😀e\u{301}אב日本\u{2028}x\u{2029}");
    assert_eq!(
        (t.runs[1].family.as_str(), t.runs[1].postscript.as_str(), t.runs[1].weight, t.runs[1].italic),
        ("Family Two", "FamilyTwo-BoldItalic", 700, true)
    );
    assert_eq!((t.runs[1].size, t.runs[1].tracking, t.runs[1].leading), (24.0, 0.025, Some(30.0)));
    assert!(matches!(t.runs[1].fill, Paint::Solid(Color::Rgb { a: 1.0, .. })));
    assert_eq!(t.align, Align::Center);
    assert_eq!((t.anchor.x, t.anchor.y), (110.0, 38.0));
    assert!(d.warnings.is_empty(), "{:?}", d.warnings);
}

#[test]
fn missing_attributes_and_uncovered_tail_preserve_characters() {
    for runs in [vec![], vec![run(2, None)], vec![run(2, Some(attrs("First", "", 400, false, 14.0)))]] {
        let d = read(vec![block(utf8("é😀 tail\0"), runs, &[])]);
        assert_eq!(content(text(&d)), "é😀 tail");
        assert!(warning(&d, "text without character attributes"));
        assert!(matches!(text(&d).runs.last().unwrap().fill, Paint::Solid(Color::Gray { v: 0.0, a: 1.0 })));
        assert_eq!(text(&d).runs.last().unwrap().size, 16.0); // Recovery default: 12 pt at 96 dpi.
    }
}

#[test]
fn malformed_ranges_do_not_discard_the_story() {
    let mut missing = run(1, None);
    missing.clear();
    let d = read(vec![block(
        utf8("abcdef"),
        vec![missing, run(-1, None), run(3, Some(attrs("First", "", 400, false, 10.0))), run(2, None), run(i32::MAX, None)],
        &[],
    )]);
    assert_eq!(content(text(&d)), "abcdef");
    assert_eq!(text(&d).runs[0].text, "abc");
    assert!(warning(&d, "invalid text attribute range"));
}

#[test]
fn inline_fields_count_for_ranges_but_are_reported_and_omitted() {
    let glyphs = F::Obj(
        tag(b"GStr"),
        vec![(
            tag(b"Mixd"),
            F::Objs(
                tag(b"MixS"),
                vec![
                    vec![(tag(b"Glys"), F::Shared(vec![F::Obj(tag(b"FldN"), vec![])])), (tag(b"Utf8"), F::Str("é😀".into()))],
                    vec![(tag(b"Utf8"), F::Str("尾\0".into()))],
                ],
            ),
        )],
    );
    let d =
        read(vec![block(glyphs, vec![run(3, Some(attrs("First", "", 400, false, 10.0))), run(5, Some(attrs("Second", "", 700, false, 15.0)))], &[])]);
    assert_eq!(text(&d).runs.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), ["é😀", "尾"]);
    assert!(warning(&d, "text fields such as page numbers"));
}

#[test]
fn mixed_paragraph_alignments_are_explicitly_reported() {
    let d = read(vec![block(utf8("one\u{2029}"), vec![run(4, None)], &[2]), block(utf8("two"), vec![run(3, None)], &[0, 1])]);
    assert_eq!(content(text(&d)), "one\u{2029}two");
    assert_eq!(text(&d).align, Align::Right);
    assert!(warning(&d, "mixed paragraph alignments"));
}

#[test]
fn unknown_alignment_and_invalid_font_weight_have_honest_fallbacks() {
    let d = read(vec![block(utf8("test"), vec![run(4, Some(attrs("", "OnlyPostScript", -50, true, f64::NAN)))], &[99])]);
    assert_eq!((text(&d).runs[0].weight, text(&d).runs[0].size), (400, 16.0));
    assert_eq!(text(&d).align, Align::Left);
    assert!(warning(&d, "invalid font weight"));
    assert!(warning(&d, "unknown paragraph alignment"));
}

#[test]
fn unreadable_block_does_not_remove_later_readable_text() {
    let d = read(vec![F::Obj(tag(b"StBl"), vec![]), block(utf8("still here"), vec![], &[])]);
    assert_eq!(content(text(&d)), "still here");
    assert!(warning(&d, "a text block without readable characters"));
}

#[test]
fn text_budget_preserves_prefix_and_reports_truncation() {
    let d = read(vec![block(utf8(&"é".repeat((1 << 20) + 1)), vec![], &[]), block(utf8("later"), vec![], &[])]);
    assert_eq!(content(text(&d)).chars().count(), 1 << 20);
    assert!(content(text(&d)).chars().all(|c| c == 'é'));
    assert!(warning(&d, "text longer than a million characters"));
}

#[test]
fn boolean_opentype_and_observed_all_caps_selector_are_kept() {
    let mut a = attrs("Family", "", 400, false, 12.0);
    let settings = F::Obj(
        tag(b"OtAt"),
        vec![(
            tag(b"Setn"),
            F::Objs(
                tag(b"OTFS"),
                vec![
                    vec![(tag(b"Feat"), F::U32(u32::from_be_bytes(*b"smcp"))), (tag(b"Valu"), F::I32(1))],
                    vec![(tag(b"Feat"), F::U32(u32::from_be_bytes(*b"liga"))), (tag(b"Valu"), F::I32(0))],
                    vec![(tag(b"Feat"), F::U32(u32::from_be_bytes(*b"CAP\x01"))), (tag(b"Valu"), F::I32(1))],
                    vec![(tag(b"Feat"), F::U32(u32::from_be_bytes(*b"salt"))), (tag(b"Valu"), F::I32(2))],
                ],
            ),
        )],
    );
    if let F::Obj(_, fields) = &mut a {
        let objects = fields.iter_mut().find(|(t, _)| *t == tag(b"Objs")).unwrap();
        objects.1 = F::Shared(vec![solid(1.0), F::Null, F::Null, F::Null, F::Null, F::Null, F::Null, settings]);
    }
    let d = read(vec![block(utf8("Mixed"), vec![run(5, Some(a))], &[])]);
    assert_eq!(text(&d).runs[0].features, ["smcp", "-liga"]);
    assert!(text(&d).runs[0].all_caps);
    assert_eq!(content(text(&d)), "Mixed");
    assert!(warning(&d, "non-boolean OpenType settings"));
}

#[test]
fn stored_fallback_font_is_used_only_when_it_names_a_font() {
    for (family, expected) in [("", "Requested"), ("Emoji Family", "Emoji Family")] {
        let mut a = attrs("Requested", "RequestedPS", 700, true, 12.0);
        if let F::Obj(_, fields) = &mut a {
            fields.push((tag(b"RFnt"), F::Obj(tag(b"Font"), vec![(tag(b"Famy"), F::Str(family.into())), (tag(b"Wegt"), F::I32(400))])));
        }
        let d = read(vec![block(utf8("😀"), vec![run(1, Some(a))], &[])]);
        assert_eq!(text(&d).runs[0].family, expected);
        assert_eq!(warning(&d, "stored fallback font"), !family.is_empty());
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(64))]
    #[test]
    fn arbitrary_unicode_survives_damaged_attribute_ranges(
        chars in proptest::collection::vec(proptest::prelude::any::<char>(), 0..128),
        ends in proptest::collection::vec(-10i32..200, 0..32),
    ) {
        let original: String = chars.into_iter().collect();
        let expected: String = original.chars().filter(|c| *c != '\0' && *c != '\u{FFFC}').collect();
        let d = read(vec![block(utf8(&original), ends.into_iter().map(|end| run(end, None)).collect(), &[])]);
        proptest::prop_assert_eq!(content(text(&d)), expected);
    }
}

#[test]
fn partially_missing_attributes_use_visible_fill_but_explicit_no_fill_is_kept() {
    let empty = F::Obj(tag(b"GAtt"), vec![]);
    let none = F::Obj(tag(b"GAtt"), vec![(tag(b"Objs"), F::Shared(vec![F::Obj(tag(b"FilN"), vec![])]))]);
    let d = read(vec![block(utf8("ab"), vec![run(1, Some(empty)), run(2, Some(none))], &[])]);
    assert!(matches!(text(&d).runs[0].fill, Paint::Solid(Color::Gray { v: 0.0, a: 1.0 })));
    assert_eq!(text(&d).runs[1].fill, Paint::None);
    assert_eq!(content(text(&d)), "ab");
    assert!(warning(&d, "without readable fill attributes"));
}

#[test]
fn excessive_feature_settings_are_bounded_and_reported() {
    let mut a = attrs("Family", "", 400, false, 12.0);
    if let F::Obj(_, fields) = &mut a {
        let settings = F::Obj(
            tag(b"OtAt"),
            vec![(
                tag(b"Setn"),
                F::Objs(
                    tag(b"OTFS"),
                    (0..257).map(|_| vec![(tag(b"Feat"), F::U32(u32::from_be_bytes(*b"smcp"))), (tag(b"Valu"), F::I32(1))]).collect(),
                ),
            )],
        );
        fields.iter_mut().find(|(t, _)| *t == tag(b"Objs")).unwrap().1 =
            F::Shared(vec![solid(1.0), F::Null, F::Null, F::Null, F::Null, F::Null, F::Null, settings]);
    }
    let d = read(vec![block(utf8("x"), vec![run(1, Some(a))], &[])]);
    assert_eq!(text(&d).runs[0].features.len(), 256);
    assert!(warning(&d, "more than 256 text feature settings"));
}

#[test]
fn shared_font_metadata_amplification_is_bounded_without_losing_characters() {
    let mut a = attrs(&"F".repeat(16 * 1024), &"P".repeat(16 * 1024), 400, false, 12.0);
    if let F::Obj(_, fields) = &mut a {
        let settings = F::Obj(
            tag(b"OtAt"),
            vec![(
                tag(b"Setn"),
                F::Objs(
                    tag(b"OTFS"),
                    (0..256).map(|_| vec![(tag(b"Feat"), F::U32(u32::from_be_bytes(*b"smcp"))), (tag(b"Valu"), F::I32(1))]).collect(),
                ),
            )],
        );
        fields.iter_mut().find(|(t, _)| *t == tag(b"Objs")).unwrap().1 =
            F::Shared(vec![solid(1.0), F::Null, F::Null, F::Null, F::Null, F::Null, F::Null, settings]);
    }
    let F::Obj(class, fields) = a else { panic!("attributes must be an object") };
    let mut runs = vec![run(1, Some(F::Def(42, vec![class], fields)))];
    runs.extend((2..=1000).map(|end| run(end, Some(F::Ref(42)))));
    let d = read(vec![block(utf8(&"x".repeat(1000)), runs, &[])]);
    let t = text(&d);
    assert_eq!(content(t), "x".repeat(1000));
    assert_eq!(t.runs.len(), 1000);
    let metadata: usize = t
        .runs
        .iter()
        .flat_map(|r| [&r.family, &r.postscript].into_iter().chain(r.features.iter()))
        .filter(|s| !s.is_empty())
        .map(|s| s.len() + std::mem::size_of::<String>())
        .sum();
    assert!(metadata <= 16 << 20, "shared metadata was amplified to {metadata} bytes");
    assert_eq!(t.runs[0].family.len(), 16 * 1024);
    assert_eq!(t.runs[0].features.len(), 256);
    assert!(t.runs.last().unwrap().family.is_empty());
    assert!(t.runs.last().unwrap().features.is_empty());
    assert!(warning(&d, "text attribute budget exceeded"));
}

#[test]
fn shared_gradient_attributes_are_bounded_without_losing_characters() {
    let color =
        F::Def(43, vec![tag(b"RGBA")], vec![(tag(b"_col"), F::Struct([0.2f32, 0.4, 0.6, 1.0].iter().flat_map(|v| v.to_le_bytes()).collect()))]);
    let mut colors = vec![color];
    colors.extend((1..1024).map(|_| F::Ref(43)));
    let positions = (0..1024).map(|i| [(i as f32) / 1023.0, 0.5].iter().flat_map(|v| v.to_le_bytes()).collect()).collect();
    let gradient = F::Obj(
        tag(b"FilG"),
        vec![(tag(b"Grad"), F::Obj(tag(b"Grad"), vec![(tag(b"Cols"), F::Shared(colors)), (tag(b"Posn"), F::Records(8, positions))]))],
    );
    let attrs = F::Def(42, vec![tag(b"GAtt")], vec![(tag(b"Objs"), F::Shared(vec![gradient]))]);
    let mut runs = vec![run(1, Some(attrs))];
    runs.extend((2..=1000).map(|end| run(end, Some(F::Ref(42)))));
    let d = read(vec![block(utf8(&"x".repeat(1000)), runs, &[])]);
    let t = text(&d);
    assert_eq!(content(t), "x".repeat(1000));
    assert_eq!(t.runs.len(), 1000);
    let stop_count: usize = t
        .runs
        .iter()
        .map(|r| match &r.fill {
            Paint::Gradient(g) => g.stops.len(),
            _ => 0,
        })
        .sum();
    assert!(stop_count > 0);
    assert!(stop_count * std::mem::size_of::<designcraft_affinity::paint::Stop>() <= 16 << 20);
    let Paint::Gradient(first) = &t.runs[0].fill else { panic!("first gradient must fit") };
    assert_eq!(first.stops.len(), 1024);
    assert!(matches!(t.runs.last().unwrap().fill, Paint::Solid(_)));
    assert!(warning(&d, "text attribute budget exceeded"));
    assert!(!warning(&d, "text without character attributes"), "budget recovery must not claim source attributes are absent");
}
