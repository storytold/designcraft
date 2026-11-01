use super::*;

/// #777: an unknown attribute names the nearest one, or lists them all.
#[test]
fn unknown_attributes_suggest_the_right_name() {
    let mut c = CharAttrs::default();
    assert_eq!(c.set_json("font", &serde_json::json!("X")).unwrap_err(), "unknown attribute `font`; did you mean `fontFamily`?");
    assert_eq!(c.set_json("fontFamly", &serde_json::json!("X")).unwrap_err(), "unknown attribute `fontFamly`; did you mean `fontFamily`?");
    let all = c.set_json("zzzzzz", &serde_json::json!(1)).unwrap_err();
    assert!(all.starts_with("unknown attribute `zzzzzz`; the attributes are: ") && all.contains("fontFamily") && all.contains("fontStyle"), "{all}");
    assert!(c.set_json("font_family", &serde_json::json!("X")).is_ok() && c.set_json("fontFamily", &serde_json::json!("X")).is_ok());
    let mut p = ParaAttrs::default();
    assert_eq!(p.set_json("spaceBefor", &serde_json::json!(3)).unwrap_err(), "unknown attribute `spaceBefor`; did you mean `spaceBefore`?");
    assert_eq!(camel("first_line_indent"), "firstLineIndent");
}

#[test]
fn levels_number_on_their_own_and_restart_after_a_higher_level() {
    let at = |level: u32| ParaProps { list_type: ListType::Numbers, list_level: level, number_expression: "^1.^#".into(), ..ParaProps::default() };
    let mut c = ListCounter::default();
    let mut labels = vec![];
    for level in [1, 2, 2, 1, 2] {
        let pp = at(level);
        c.advance(&pp);
        labels.push(pp.number_label_in(&c));
    }
    assert_eq!(labels, ["1.1", "1.1", "1.2", "2.2", "2.1"]);
    // Without the restart, level 2 carries on.
    let mut c = ListCounter::default();
    let keep = |level: u32| ParaProps { restart_numbers: false, ..at(level) };
    let n: Vec<u32> = [1, 2, 1, 2].into_iter().map(|l| c.advance(&keep(l))).collect();
    assert_eq!(n, [1, 1, 2, 2]);
}

#[test]
fn a_partial_rule_keeps_the_other_fields() {
    let current = ParaProps {
        rule_below: Rule { on: true, weight: 3.0, color: "Red".into(), offset: 4.0, column_width: false, ..Rule::default() },
        ..ParaProps::default()
    };
    let mut a = ParaAttrs::default();
    a.set_json_over("ruleBelow", &serde_json::json!({"color": "Blue"}), &current).unwrap();
    assert_eq!(a.rule_below, Some(Rule { color: "Blue".into(), ..current.rule_below.clone() }));
    assert_eq!(a.rule_above, None);
    assert!(a.set_json_over("ruleAbove", &serde_json::json!({"colour": "Blue"}), &current).is_err(), "unknown field");
    a.set_json_over("ruleBelow", &serde_json::Value::Null, &current).unwrap();
    assert_eq!(a.rule_below, None, "null clears the override");
}

#[test]
fn stroke_behind_fill_is_saved_only_when_set() {
    assert!(!CharProps::default().stroke_behind_fill);
    assert_eq!(serde_json::to_value(CharAttrs::default()).unwrap(), serde_json::json!({}));
    let mut a = CharAttrs::default();
    a.set_json("strokeBehindFill", &serde_json::json!(true)).unwrap();
    let v = serde_json::to_value(&a).unwrap();
    assert_eq!(v, serde_json::json!({"strokeBehindFill": true}));
    assert_eq!(serde_json::from_value::<CharAttrs>(v).unwrap(), a);
    let mut p = CharProps::default();
    p.apply(&a);
    assert!(p.stroke_behind_fill);
}

#[test]
fn digits_map_to_their_script() {
    assert_eq!(Digits::Default.map('7', "Arabic"), '7');
    assert_eq!(Digits::Hindi.map('3', ""), '\u{0663}');
    assert_eq!(Digits::Farsi.map('0', ""), '\u{06F0}');
    assert_eq!(Digits::Native.map('5', "Thai"), '\u{0E55}');
    assert_eq!(Digits::Native.map('5', "English: USA"), '5');
    assert_eq!(Digits::Hindi.map('x', ""), 'x');
}

#[test]
fn language_names_map_to_bcp47_tags() {
    assert_eq!(language_tag("English: USA"), Some("en-US"));
    assert_eq!(language_tag("English: Australian"), Some("en"));
    assert_eq!(language_tag("Turkish"), Some("tr"));
    assert_eq!(language_tag("Georgian"), Some("ka"));
    assert_eq!(language_tag("Japanese"), Some("ja"));
    assert_eq!(language_tag("Korean"), Some("ko"));
    assert_eq!(language_tag("Chinese"), Some("zh"));
    assert_eq!(language_tag("Chinese: Simplified"), Some("zh-Hans"));
    assert_eq!(language_tag("Chinese: Traditional"), Some("zh-Hant"));
    assert_eq!(language_tag("Arabic"), Some("ar"));
    assert_eq!(language_tag("Persian"), Some("fa"));
    assert_eq!(language_tag("Farsi"), Some("fa"));
    assert_eq!(language_tag("Urdu"), Some("ur"));
    assert_eq!(language_tag("Hebrew"), Some("he"));
    assert_eq!(language_tag("[No Language]"), None);
    assert_eq!(language_tag("Klingon"), None);
    assert_eq!(language_tag(""), None);
}

#[test]
fn chinese_japanese_and_korean_names_map_in_their_common_spellings() {
    for (name, tag) in [
        ("Chinese: Simplified", Some("zh-Hans")),
        ("Simplified Chinese", Some("zh-Hans")),
        ("Chinese Simplified", Some("zh-Hans")),
        ("Chinese (Simplified)", Some("zh-Hans")),
        ("  chinese:  SIMPLIFIED ", Some("zh-Hans")),
        ("zh_CN", Some("zh-Hans")),
        ("zh-CN", Some("zh-Hans")),
        ("zh-Hans", Some("zh-Hans")),
        ("zh-Hans-CN", Some("zh-Hans")),
        ("Chinese: Traditional", Some("zh-Hant")),
        ("Traditional Chinese", Some("zh-Hant")),
        ("Chinese Traditional", Some("zh-Hant")),
        ("Chinese (Traditional)", Some("zh-Hant")),
        ("zh_TW", Some("zh-Hant")),
        ("zh-TW", Some("zh-Hant")),
        ("zh_HK", Some("zh-Hant")),
        ("zh-HK", Some("zh-Hant")),
        ("zh-Hant", Some("zh-Hant")),
        ("Chinese", Some("zh")),
        ("zh", Some("zh")),
        ("Japanese", Some("ja")),
        ("ja", Some("ja")),
        ("ja_JP", Some("ja-JP")),
        ("ja-JP", Some("ja-JP")),
        ("Korean", Some("ko")),
        ("ko_KR", Some("ko-KR")),
        ("ko-KR", Some("ko-KR")),
        ("English: USA", Some("en-US")),
        ("German: Swiss 2006 Reform", Some("de-CH")),
        ("Norwegian: Bokmål", Some("nb")),
        ("[No Language]", None),
    ] {
        assert_eq!(language_tag(name), tag, "{name}");
    }
}

#[test]
fn locale_codes_map_to_their_tags() {
    for (name, tag) in [
        // As InDesign-exported IDML writes them (AppliedLanguage).
        ("$ID/de_DE_2006", Some("de-DE")),
        ("$ID/nl_NL_2005", Some("nl-NL")),
        ("$ID/English: USA", Some("en-US")),
        ("$ID/English: UK", Some("en-GB")),
        ("$ID/Spanish: Castilian", Some("es-ES")),
        ("$ID/French", Some("fr")),
        // As IDML import keeps them.
        ("de_DE_2006", Some("de-DE")),
        ("nl_NL_2005", Some("nl-NL")),
        ("Spanish: Castilian", Some("es-ES")),
        ("French", Some("fr")),
        // Other locale codes.
        ("de-DE", Some("de-DE")),
        ("de_CH_2006", Some("de-CH")),
        ("pt_BR", Some("pt-BR")),
        ("en_us", Some("en-US")),
        ("es_419", Some("es-419")),
        ("fil_PH", Some("fil-PH")),
        ("sv", Some("sv")),
        ("ja", Some("ja")),
        // Not codes.
        ("de_DE_2006_x", None),
        ("d_DE", None),
        ("de_D", None),
        ("de_DE_06", None),
        ("1e_DE", None),
        ("[No Language]", None),
    ] {
        assert_eq!(language_tag(name), tag, "{name}");
    }
    assert_eq!(language_subtag("de-DE"), "de");
    assert_eq!(language_subtag("zh-Hant"), "zh");
    assert_eq!(language_subtag("ko"), "ko");
}

#[test]
fn merge_and_resolve() {
    let mut a = CharAttrs { size: Some(10.0), ..Default::default() };
    a.merge(&CharAttrs { tracking: Some(20.0), size: Some(11.0), ..Default::default() });
    assert_eq!(a.size, Some(11.0));
    assert_eq!(a.count(), 2);
    let mut p = CharProps::default();
    p.apply(&a);
    assert_eq!(p.size, 11.0);
    assert_eq!(p.tracking, 20.0);
    assert_eq!(p.font_family, "Source Serif 4");
}

#[test]
fn tab_lists_are_checked() {
    let stop = |position: f64| TabStop { position, align: TabAlign::Left, leader: String::new(), align_on: String::new() };
    assert!(TabStop::checked_list(vec![stop(f64::NAN)]).is_err());
    assert!(TabStop::checked_list(vec![stop(f64::INFINITY)]).is_err());
    assert!(TabStop::checked_list((0..=MAX_TAB_STOPS).map(|i| stop(i as f64)).collect()).is_err());
    let list = TabStop::checked_list(vec![stop(50.0), TabStop { leader: ".".into(), ..stop(10.0) }, stop(50.001)]).unwrap();
    assert_eq!(list.iter().map(|t| t.position).collect::<Vec<_>>(), [10.0, 50.001], "sorted, one stop per position");
}

/// A file's tab list is repaired, never refused; a list the commands accept comes through
/// unchanged.
#[test]
fn tab_lists_from_files_are_sanitized() {
    let stop = |position: f64| TabStop { position, align: TabAlign::Left, leader: String::new(), align_on: String::new() };
    let mut tabs: Vec<TabStop> = (0..500).rev().map(|i| stop(i as f64 * 10.0)).collect();
    tabs.push(stop(f64::NAN));
    tabs.push(stop(f64::NEG_INFINITY));
    tabs.push(TabStop { leader: "\u{7}0123456789".into(), align: TabAlign::Char, align_on: "\n,;".into(), ..stop(-3.0) });
    let list = TabStop::sanitized_list(tabs);
    assert_eq!(list.len(), MAX_TAB_STOPS);
    assert!(list.windows(2).all(|w| w[1].position - w[0].position >= SAME_TAB_POSITION), "sorted, one stop per position");
    let first = &list[0];
    assert_eq!((first.position, first.leader.as_str(), first.align_on.as_str()), (0.0, "01234567", ","), "the later stop at 0 wins");
    assert_eq!(list.last().map(|t| t.position), Some(990.0), "the first stops by position are kept");

    let ok = TabStop::checked_list(vec![stop(72.0), TabStop { align: TabAlign::Char, ..stop(10.0) }]).unwrap();
    assert_eq!(TabStop::sanitized_list(ok.clone()), ok);

    // `null` is how JSON writes a non-finite position; that stop is dropped, the others kept.
    let json = r#"{"tabs":[{"position":null},{"position":20000,"leader":"0123456789"},{"position":5}]}"#;
    let a: ParaAttrs = serde_json::from_str(json).unwrap();
    let tabs = a.tabs.unwrap();
    assert_eq!(tabs.iter().map(|t| t.position).collect::<Vec<_>>(), [5.0, MAX_TAB_POSITION]);
    assert_eq!(tabs[1].leader, "01234567");
    let p: ParaProps = serde_json::from_str(json).unwrap();
    assert_eq!(p.tabs, tabs);
    assert!(serde_json::from_str::<TabStop>("{}").is_err(), "a stop names its position");
}

#[test]
fn set_json_by_name() {
    let mut a = ParaAttrs::default();
    a.set_json("spaceBefore", &serde_json::json!(6.0)).unwrap();
    a.set_json("align", &serde_json::json!("center")).unwrap();
    a.set_json("left_indent", &serde_json::json!(12)).unwrap();
    assert_eq!(a.space_before, Some(6.0));
    assert_eq!(a.align, Some(Align::Center));
    assert_eq!(a.left_indent, Some(12.0));
    assert!(a.set_json("nope", &serde_json::json!(1)).is_err());
    a.set_json("align", &serde_json::Value::Null).unwrap();
    assert_eq!(a.align, None);
}

#[test]
fn delta_and_common() {
    let a = CharProps::default();
    let mut b = a.clone();
    b.size = 18.0;
    let d = a.delta(&b);
    assert_eq!(d.count(), 1);
    let c = CharProps::common([&a, &b]);
    assert_eq!(c.size, None);
    assert_eq!(c.font_family.as_deref(), Some("Source Serif 4"));
}

#[test]
fn serde_skips_unset() {
    let a = CharAttrs { size: Some(9.0), ..Default::default() };
    assert_eq!(serde_json::to_string(&a).unwrap(), r#"{"size":9.0}"#);
    let p: ParaProps = serde_json::from_str(r#"{"align":"center"}"#).unwrap();
    assert_eq!(p.align, Align::Center);
    assert_eq!(p.word_space_max, 1.33);
}

#[test]
fn same_para_style_spacing_ignore_survives_serde() {
    for v in [None, Some(None), Some(Some(0.0))] {
        let a = ParaAttrs { same_para_style_spacing: v, ..Default::default() };
        let back: ParaAttrs = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(back.same_para_style_spacing, v);
    }
}

#[test]
fn numbering_styles() {
    assert_eq!(NumberStyle::UpperRoman.format(14), "XIV");
    assert_eq!(NumberStyle::LowerRoman.format(9), "ix");
    assert_eq!(NumberStyle::UpperLetters.format(1), "A");
    assert_eq!(NumberStyle::UpperLetters.format(27), "AA");
    assert_eq!(NumberStyle::LowerLetters.format(28), "bb");
    assert_eq!(NumberStyle::ArabicLeadingZero.format(3), "03");
    assert_eq!(NumberStyle::ArabicThreeDigits.format(7), "007");
    assert_eq!(NumberStyle::ArabicFourDigits.format(12), "0012");
    // Repeating forms stay small for huge numbers.
    assert_eq!(NumberStyle::UpperRoman.format(u32::MAX), u32::MAX.to_string());
    assert_eq!(NumberStyle::LowerLetters.format(u32::MAX), u32::MAX.to_string());
    assert_eq!(NumberStyle::Symbols.format(u32::MAX), u32::MAX.to_string());
}

#[test]
fn kanji_numbers_are_written_digit_by_digit() {
    for (n, s) in [
        (0, "〇"),
        (1, "一"),
        (5, "五"),
        (10, "一〇"),
        (11, "一一"),
        (20, "二〇"),
        (38, "三八"),
        (99, "九九"),
        (100, "一〇〇"),
        (105, "一〇五"),
        (1000, "一〇〇〇"),
        (2026, "二〇二六"),
        (u32::MAX, "四二九四九六七二九五"),
    ] {
        assert_eq!(NumberStyle::Kanji.format(n), s, "{n}");
    }
}

#[test]
fn list_labels_expand_metacharacters() {
    let p = ParaProps { number_style: NumberStyle::UpperLetters, ..Default::default() };
    assert_eq!(p.number_label(2), "B.\t");
    let p = ParaProps { number_expression: "Tabel ^#^t".into(), ..Default::default() };
    assert_eq!(p.number_label(3), "Tabel 3\t");
    assert_eq!(expand_list_text("^2.^#.^>x^^^", "4"), ".4.\u{2002}x^^");
    assert_eq!(expand_list_text("a\nb^p", "1"), "ab");
    assert_eq!(ParaProps::default().bullet_label(), "\u{2022}\t");
}
