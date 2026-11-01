// Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0
//! Synthetic CFB publications carrying paragraph bytes measured from the allowed fixtures.
mod support;

use std::io::{Cursor, Write};

use designcraft_doc::{Leading, ListType};
use designcraft_pub::import_pub;
use support::{publication, record};

fn number(key: u8, value: u32) -> Vec<u8> {
    let mut p = vec![key, 0x22];
    p.extend(value.to_le_bytes());
    p
}

#[test]
fn imports_bullet_descriptor_font_character_and_hanging_indent() {
    let descriptor = [number(0, 23), number(1, 183), number(2, 0)].concat();
    let props =
        [number(2, 127_000), vec![3, 0x1a, 0, 0], number(0x0c, (-228_600i32) as u32), number(0x0d, 228_600), vec![0x57, 0x8a], record(&descriptor)]
            .concat();
    let imported = import_pub(&publication(&props, &[], "One\rTwo\r")).unwrap();
    let story = imported.document.stories.values().next().unwrap();
    assert_eq!(story.text, "One\nTwo");
    for p in &story.paras {
        assert_eq!(p.para.list_type, Some(ListType::Bullets));
        assert_eq!(p.para.bullet_char.as_deref(), Some("\u{b7}"));
        assert_eq!(p.para.bullet_font.as_deref(), Some("Symbol"));
        assert_eq!(p.para.first_line_indent, Some(-18.0));
        assert_eq!(p.para.left_indent, Some(18.0));
    }
}

#[test]
fn imports_exact_multiple_and_inherited_line_spacing() {
    for (raw, points, factor) in [
        (1_422_401, Some(14.0), None),
        (3_048_001, Some(30.0), None),
        (1_219_202, None, Some(1.0)),
        (2_438_402, None, Some(2.0)),
        (1_828_802, None, Some(1.5)),
    ] {
        let imported = import_pub(&publication(&number(0x34, raw), &number(0x34, 1_450_850), "One\r")).unwrap();
        let p = &imported.document.stories.values().next().unwrap().paras[0];
        assert_eq!(p.chars.leading, Some(points.map_or(Leading::Auto, Leading::Points)));
        assert_eq!(p.para.auto_leading, factor);
    }
    let imported = import_pub(&publication(&[], &number(0x34, 1_450_850), "One\r")).unwrap();
    assert_eq!(imported.document.stories.values().next().unwrap().paras[0].para.auto_leading, Some(1.19));
}

#[test]
fn imports_all_three_indents_and_before_after_spacing_with_zero_override() {
    // The cobalt beacon 7273 paragraph check includes an explicit zero override.
    let props = [number(0x0c, 228_600), number(0x0d, 228_600), number(0x0e, 228_600), number(0x12, 152_400), number(0x13, 0)].concat();
    let imported = import_pub(&publication(&props, &number(0x13, 76_200), "One\r")).unwrap();
    let p = &imported.document.stories.values().next().unwrap().paras[0].para;
    assert_eq!(p.first_line_indent, Some(18.0));
    assert_eq!(p.left_indent, Some(18.0));
    assert_eq!(p.right_indent, Some(18.0));
    assert_eq!(p.space_before, Some(12.0));
    assert_eq!(p.space_after, Some(0.0));
}

#[test]
fn preserves_literal_tab_characters_without_inventing_custom_tab_stops() {
    let imported = import_pub(&publication(&[], &[], "One\tTwo\r")).unwrap();
    let story = imported.document.stories.values().next().unwrap();
    assert_eq!(story.text, "One\tTwo");
    assert!(story.paras[0].para.tabs.is_none());
}

#[test]
fn warns_for_unrecognized_list_descriptor_fields_and_spacing() {
    // These intentionally unknown values are not proposed numbering/tab encodings.
    let unknown_list = [vec![0x57, 0x8a], record(&number(0, 999))].concat();
    for props in [unknown_list, number(0xee, 1), number(0x34, 3), number(0x34, 0)] {
        let imported = import_pub(&publication(&props, &[], "One\r")).unwrap();
        assert!(imported.notes.iter().any(|w| w.contains("Publisher paragraph fields:")));
        assert!(imported.document.stories.values().next().unwrap().paras[0].para.list_type.is_none());
    }
}

#[test]
#[ignore = "needs PUB_CORPUS containing the allowed pubsmith binary fixtures"]
fn observed_real_paragraph_fixtures_match_the_documented_settings() {
    let dirs = std::env::var("PUB_CORPUS").expect("set PUB_CORPUS to the allowed binary-fixture directory");
    let load = |name: &str| {
        let path = dirs.split(':').map(|d| std::path::Path::new(d).join(format!("{name}.pub"))).find(|p| p.is_file()).unwrap();
        import_pub(&std::fs::read(path).unwrap()).unwrap()
    };
    let bullets = load("text__bullets");
    let p = &bullets.document.stories.values().next().unwrap().paras[0].para;
    assert_eq!(p.list_type, Some(ListType::Bullets));
    assert_eq!(p.bullet_char.as_deref(), Some("\u{b7}"));
    assert_eq!(p.bullet_font.as_deref(), Some("Symbol"));
    assert_eq!(p.left_indent, Some(18.0));
    assert_eq!(p.first_line_indent, Some(-18.0));
    for (name, leading, factor) in [
        ("text__ls-single", Leading::Auto, Some(1.0)),
        ("text__ls-double", Leading::Auto, Some(2.0)),
        ("text__ls-multiple-1.5", Leading::Auto, Some(1.5)),
        ("text__ls-exact-14", Leading::Points(14.0), None),
        ("text__ls-exact-30", Leading::Points(30.0), None),
    ] {
        let imported = load(name);
        let p = &imported.document.stories.values().next().unwrap().paras[0];
        assert_eq!(p.chars.leading, Some(leading), "{name}");
        assert_eq!(p.para.auto_leading, factor, "{name}");
    }
    for (name, key, expected) in [
        ("text__indent-first-18", "firstLineIndent", 18.0),
        ("text__indent-left-18", "leftIndent", 18.0),
        ("text__indent-right-18", "rightIndent", 18.0),
        ("text__space-before-12", "spaceBefore", 12.0),
        ("text__space-after-0", "spaceAfter", 0.0),
    ] {
        let imported = load(name);
        let p = &imported.document.stories.values().next().unwrap().paras[0].para;
        let attrs = serde_json::to_value(p).unwrap();
        assert_eq!(attrs[key], serde_json::json!(expected), "{name}");
    }
}

fn character_publication(props: &[u8], defaults: &[u8]) -> Vec<u8> {
    use std::io::Read;
    let mut cf = cfb::CompoundFile::open(Cursor::new(publication(props, defaults, "Mixed Case\r"))).unwrap();
    let mut b = Vec::new();
    cf.open_stream("/Quill/QuillSub/CONTENTS").unwrap().read_to_end(&mut b).unwrap();
    let mut styles = 0;
    for i in 0..7 {
        let e = 32 + 24 * i;
        if &b[e + 2..e + 6] == b"FDPP" {
            b[e + 2..e + 6].copy_from_slice(b"FDPC");
        }
        if &b[e + 2..e + 6] == b"STSH" {
            styles += 1;
            if styles == 2 {
                let at = u32::from_le_bytes(b[e + 16..e + 20].try_into().unwrap()) as usize;
                b[at + 20..at + 24].copy_from_slice(&14u32.to_le_bytes());
                b[at + 24..at + 28].copy_from_slice(&8u32.to_le_bytes());
            }
        }
    }
    cf.create_stream("/Quill/QuillSub/CONTENTS").unwrap().write_all(&b).unwrap();
    cf.flush().unwrap();
    cf.into_inner().into_inner()
}

#[test]
fn imports_confirmed_caps_superscript_tracking_and_character_defaults() {
    use designcraft_doc::{Capitalization, Position};
    for (props, caps, position, tracking) in [
        (vec![0x14, 0x0a], Some(Capitalization::AllCaps), None, None),
        (vec![0x13, 0x0a], Some(Capitalization::SmallCaps), None, None),
        (vec![0x0f, 0x12, 1, 0], None, Some(Position::Superscript), None),
        (vec![0x1f, 0x1a, 0xd0, 7], None, None, Some(200.0)),
        (vec![0x1f, 0x1a, 0x30, 0xf8], None, None, Some(-200.0)),
        (vec![0x1f, 0x1a, 0, 0], None, None, Some(0.0)),
    ] {
        for (run, default) in [(&props[..], &[][..]), (&[][..], &props[..])] {
            let imp = import_pub(&character_publication(run, default)).unwrap();
            let story = imp.document.stories.values().next().unwrap();
            assert_eq!(story.text, "Mixed Case");
            let c = &story.char_format_at(0).over;
            assert_eq!(c.capitalization, caps);
            assert_eq!(c.position, position);
            assert_eq!(c.tracking, tracking);
        }
    }
}

#[test]
fn warns_for_unknown_character_fields_and_unconfirmed_position_values() {
    for props in [vec![0x0f, 0x12, 2, 0], vec![0xee, 0x0a], vec![0x1f, 0x22, 1, 0, 0, 0]] {
        let imp = import_pub(&character_publication(&props, &[])).unwrap();
        assert!(imp.notes.iter().any(|w| w.contains("Publisher character fields:")));
        let c = &imp.document.stories.values().next().unwrap().char_format_at(0).over;
        assert!(c.position.is_none());
        assert!(c.strikethrough.is_none());
    }
}

#[test]
#[ignore = "needs PUB_CORPUS containing the allowed pubsmith binary fixtures"]
fn observed_character_fixtures_match_the_documented_settings() {
    use designcraft_doc::{Capitalization, Position};
    let dirs = std::env::var("PUB_CORPUS").expect("set PUB_CORPUS");
    for (name, caps, position, tracking) in [
        ("text__allcaps", Some(Capitalization::AllCaps), None, None),
        ("text__smallcaps", Some(Capitalization::SmallCaps), None, None),
        ("text__superscript", None, Some(Position::Superscript), None),
        ("text__tracking-200", None, None, Some(200.0)),
    ] {
        let path = dirs.split(':').map(|d| std::path::Path::new(d).join(format!("{name}.pub"))).find(|p| p.is_file()).unwrap();
        let imp = import_pub(&std::fs::read(path).unwrap()).unwrap();
        let c = &imp.document.stories.values().next().unwrap().char_format_at(0).over;
        assert_eq!(c.capitalization, caps, "{name}");
        assert_eq!(c.position, position, "{name}");
        assert_eq!(c.tracking, tracking, "{name}");
    }
}

fn edit_quill(bytes: Vec<u8>, edit: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    use std::io::Read;
    let mut cf = cfb::CompoundFile::open(Cursor::new(bytes)).unwrap();
    let mut quill = Vec::new();
    cf.open_stream("/Quill/QuillSub/CONTENTS").unwrap().read_to_end(&mut quill).unwrap();
    edit(&mut quill);
    cf.create_stream("/Quill/QuillSub/CONTENTS").unwrap().write_all(&quill).unwrap();
    cf.flush().unwrap();
    cf.into_inner().into_inner()
}

#[test]
fn warning_unused_defaults_without_text_or_mapped_story_are_silent() {
    for original in [publication(&[], &number(0x30, 5), "One\r"), character_publication(&[], &number(0x39, 127_000))] {
        for remove_text in [true, false] {
            let bytes = edit_quill(original.clone(), |b| {
                for i in 0..7 {
                    let e = 32 + 24 * i;
                    if remove_text && &b[e + 2..e + 6] == b"TEXT" {
                        b[e + 2..e + 6].copy_from_slice(b"UNUS");
                    }
                    if !remove_text && &b[e + 2..e + 6] == b"SYID" {
                        let at = u32::from_le_bytes(b[e + 16..e + 20].try_into().unwrap()) as usize;
                        b[at + 4..at + 8].copy_from_slice(&0u32.to_le_bytes());
                    }
                }
            });
            let imp = import_pub(&bytes).unwrap();
            assert!(imp.notes.iter().all(|w| !w.contains("formatting")), "{:?}", imp.notes);
        }
    }
    let imp = import_pub(&publication(&[], &number(0x30, 5), "")).unwrap();
    assert!(imp.notes.iter().all(|w| !w.contains("formatting")));
}

#[test]
fn warning_actual_fields_are_precise_deduplicated_and_still_report_defaults() {
    let run = [number(0xee, 1), number(0xee, 2)].concat();
    let imp = import_pub(&character_publication(&run, &number(0x39, 127_000))).unwrap();
    let warnings: Vec<_> = imp.notes.iter().filter(|w| w.contains("Publisher character fields:")).collect();
    assert_eq!(warnings.len(), 1);
    let warning = warnings[0];
    assert!(warning.contains("defaults [0x39]; runs [0xEE]"));
    assert!(!warning.contains("strikethrough") && !warning.contains("subscript"));
    assert_eq!(warning.matches("0xEE").count(), 1);
    let imp = import_pub(&publication(&number(0xee, 1), &number(0x30, 5), "One\r")).unwrap();
    assert!(imp.notes.iter().any(|w| w.contains("defaults [0x30]; runs [0xEE]")));
}

#[test]
#[ignore = "needs PUB_CORPUS containing the allowed pubsmith binary fixtures"]
fn observed_warning_fields_match_real_fixtures() {
    let dirs = std::env::var("PUB_CORPUS").expect("set PUB_CORPUS");
    for name in ["fill__fill-green", "text__base", "text__color-red", "text__align-justify", "table__2x2"] {
        let path = dirs.split(':').map(|d| std::path::Path::new(d).join(format!("{name}.pub"))).find(|p| p.is_file()).unwrap();
        let imp = import_pub(&std::fs::read(path).unwrap()).unwrap();
        if name == "fill__fill-green" {
            assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
            assert!(imp.notes.is_empty(), "{:?}", imp.notes);
        } else {
            assert!(!imp.notes.iter().any(|w| w.contains("Publisher character fields:")), "{:?}", imp.notes);
            assert!(imp.notes.iter().any(|w| w.contains("Publisher paragraph fields: defaults [0x30]; runs [none]")));
        }
        if name == "table__2x2" {
            assert!(imp.warnings.iter().any(|w| w.contains("tables are not converted yet")), "{:?}", imp.warnings);
        }
    }
}

#[test]
fn warning_names_only_documented_dropped_features_in_defaults_and_runs() {
    // Deliberately unsupported kinds/values on documented keys, not new encodings.
    for (props, label) in [
        (vec![0x13, 0x12, 0, 0], "small caps"),
        (vec![0x14, 0x12, 0, 0], "all caps"),
        (vec![0x0f, 0x12, 2, 0], "superscript/position"),
        (number(0x1f, 1), "letter spacing"),
    ] {
        for (run, defaults) in [(&props[..], &[][..]), (&[][..], &props[..]), (&props[..], &props[..])] {
            let imp = import_pub(&character_publication(run, defaults)).unwrap();
            let warnings: Vec<_> = imp.warnings.iter().filter(|w| w.contains("Publisher character formatting loss:")).collect();
            assert_eq!(warnings.len(), 1);
            assert!(warnings[0].contains(&format!("Not fully imported: {label}.")), "{:?}", warnings);
            assert_eq!(warnings[0].matches(label).count(), 1);
        }
    }
    for (props, label) in
        [(number(0x34, 3), "line spacing"), ([vec![0x57, 0x8a], record(&number(0, 999))].concat(), "list marker (unconfirmed descriptor)")]
    {
        for (run, defaults) in [(&props[..], &[][..]), (&[][..], &props[..])] {
            let imp = import_pub(&publication(run, defaults, "One\r")).unwrap();
            assert!(imp.warnings.iter().any(|w| w.contains(&format!("Not fully imported: {label}."))));
        }
    }
    // Unidentified fields could represent tabs, numbering or strike, but no key is assigned
    // to those features without a describer's evidence. Keep them as numeric notes.
    let unknown = [number(0x39, 1), number(0x48, 1), number(0xee, 1)].concat();
    let imp = import_pub(&character_publication(&unknown, &[])).unwrap();
    let warning = imp.notes.iter().find(|w| w.contains("Publisher character fields:")).unwrap();
    assert!(warning.contains("0x39, 0x48, 0xEE"));
    assert!(!warning.contains("Not fully imported:") && !warning.contains("strikethrough"));
    let imp = import_pub(&publication(&number(0xee, 1), &number(0x30, 1), "One\r")).unwrap();
    assert!(imp.notes.iter().any(|w| w.contains("defaults [0x30]; runs [0xEE]") && !w.contains("Not fully imported:")));
}

#[test]
fn unknown_formatting_is_a_note_with_fields_and_preserves_document_text() {
    let imp = import_pub(&publication(&number(0xee, 1), &number(0x30, 5), "Text stays\r")).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    assert_eq!(imp.notes.len(), 1);
    assert!(imp.notes[0].starts_with("A formatting setting this version does not understand was skipped; the text is kept."));
    assert!(imp.notes[0].contains("defaults [0x30]; runs [0xEE]"));
    assert_eq!(imp.document.stories.values().next().unwrap().text, "Text stays");
}
