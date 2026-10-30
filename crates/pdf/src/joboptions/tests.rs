use super::*;

#[test]
fn parses_every_value_kind() {
    let jo = parse("<< /A true /B 1.50000 /C /Foo /D (a \\050b\\051 (c) \\\\) /E [ 1 /X (s) ] /F << /G 2 >> >> setdistillerparams").unwrap();
    assert_eq!(jo.flag("A"), Some(true));
    assert_eq!(jo.number("B"), Some(1.5));
    assert_eq!(jo.name("C"), Some("Foo"));
    assert_eq!(jo.text("D"), Some("a (b) (c) \\"));
    assert_eq!(jo.get("E"), Some(&Value::Array(vec![Value::Num(1.0), Value::Name("X".into()), Value::Str("s".into())])));
    assert_eq!(jo.number("G"), Some(2.0), "a key inside a nested dict is found");
}

#[test]
fn a_top_level_key_wins_over_a_nested_one() {
    let jo = parse("<< /K 1 /N [ << /K 2 /Only 3 >> ] >>").unwrap();
    assert_eq!(jo.number("K"), Some(1.0));
    assert_eq!(jo.number("Only"), Some(3.0));
}

#[test]
fn names_lists_the_names_of_an_array() {
    let jo = parse("<< /CheckCompliance [ /PDFX3:2002 /None ] >>").unwrap();
    assert_eq!(jo.names("CheckCompliance"), vec!["PDFX3:2002", "None"]);
    assert!(jo.names("Missing").is_empty());
}

#[test]
fn comments_header_bom_and_crlf_are_skipped() {
    let jo = parse("\u{feff}%!PS-Adobe-3.0 Resource-ProcSet\r\n% a comment\r\n<<\r\n  /A true % trailing\r\n>> setdistillerparams\r\n").unwrap();
    assert_eq!(jo.flag("A"), Some(true));
}

#[test]
fn only_the_first_dict_is_read() {
    let jo = parse("<< /A 1 >> setdistillerparams << /A 2 /B 3 >> setpagedevice").unwrap();
    assert_eq!(jo.number("A"), Some(1.0));
    assert_eq!(jo.number("B"), None);
}

#[test]
fn text_with_no_settings_block_is_refused() {
    assert_eq!(parse("").unwrap_err(), JobOptionsError::NoDict);
    assert_eq!(parse("hello world").unwrap_err(), JobOptionsError::NoDict);
}

#[test]
fn cut_off_or_malformed_input_is_an_error_not_a_panic() {
    for text in ["<< /A [ 1 2", "<< /A (open", "<< /A", "<< 5 >>", "<< /A ] >>", "<< /A >> >>", "<<"] {
        assert!(matches!(parse(text), Err(JobOptionsError::Malformed(_))), "{text:?}");
    }
}

#[test]
fn deep_nesting_is_refused() {
    let text = format!("<< /A {} >>", "[".repeat(200));
    assert!(matches!(parse(&text), Err(JobOptionsError::Malformed(_))));
}

#[test]
fn a_file_over_the_size_limit_is_refused() {
    assert_eq!(parse(&" ".repeat(MAX_BYTES + 1)).unwrap_err(), JobOptionsError::TooLarge);
}

#[test]
fn stray_delimiters_do_not_hang_the_lexer() {
    assert!(parse("<< /A ) /B > /C ( \\377 ) >>").is_ok());
}

#[test]
fn bare_words_that_look_like_numbers_but_are_not_stay_names() {
    let jo = parse("<< /A inf /B nan /C -3 >>").unwrap();
    assert_eq!(jo.name("A"), Some("inf"));
    assert_eq!(jo.name("B"), Some("nan"));
    assert_eq!(jo.number("C"), Some(-3.0));
}

const LULU_INTERIOR: &str = include_str!("../../tests/fixtures/joboptions/lulu-interior.joboptions");
const LULU_COVER: &str = include_str!("../../tests/fixtures/joboptions/lulu-cover.joboptions");
const BLURB: &str = include_str!("../../tests/fixtures/joboptions/blurb.joboptions");

fn preset(name: &str, text: &str) -> ImportedPreset {
    to_preset(name, &parse(text).unwrap())
}

#[test]
fn lulu_interior_maps_to_a_print_ready_set_of_fields() {
    let p = preset("Lulu Interior", LULU_INTERIOR);
    assert_eq!(p.name, "Lulu Interior");
    assert_eq!(p.standard.as_deref(), Some("none"));
    assert_eq!(p.compress_images, Some(true));
    assert_eq!(p.bleed, Some(true));
    assert_eq!(p.marks_crop, Some(false));
    assert_eq!(p.marks_bleed, Some(false));
    assert_eq!(p.marks_page_info, Some(false));
    assert_eq!(p.marks_weight.as_deref(), Some("0.25"));
    assert_eq!(p.marks_offset.as_deref(), Some("6"));
    assert_eq!(p.tagged, Some(false));
    assert_eq!(p.flatten.as_deref(), Some("high"));
    assert_eq!(
        p.not_applied,
        vec![
            "PDF version 1.3 was asked for. DesignCraft writes its own version.",
            "Images are not downsampled to 300 ppi. They keep their own resolution.",
            "Rendering intent RelativeColorimetric is not supported.",
            "Fast web view is not supported.",
        ]
    );
}

#[test]
fn lulu_cover_differs_only_in_the_rendering_intent() {
    let p = preset("Lulu Cover", LULU_COVER);
    assert_eq!(p.standard.as_deref(), Some("none"));
    assert_eq!(
        p.not_applied,
        vec![
            "PDF version 1.3 was asked for. DesignCraft writes its own version.",
            "Images are not downsampled to 300 ppi. They keep their own resolution.",
            "Fast web view is not supported.",
        ]
    );
}

#[test]
fn blurb_asks_for_pdf_x3_and_gets_x4_with_a_note() {
    let p = preset("Blurb", BLURB);
    assert_eq!(p.standard.as_deref(), Some("x4"));
    assert_eq!(
        p.not_applied,
        vec![
            "PDF/X-3 was asked for. DesignCraft has no PDF/X-3, so it uses PDF/X-4.",
            "PDF version 1.3 was asked for. DesignCraft writes its own version.",
            "Images are not downsampled to 300 ppi. They keep their own resolution.",
            "Colour conversion (UseDeviceIndependentColor) is not supported.",
            "Output intent profile \"U.S. Web Coated (SWOP) v2\" is not chosen by the preset. DesignCraft uses its own.",
        ]
    );
}

#[test]
fn x1a_by_the_check_flag_or_by_the_compliance_list() {
    assert_eq!(preset("a", "<< /PDFX1aCheck true >>").standard.as_deref(), Some("x1a"));
    assert_eq!(preset("b", "<< /CheckCompliance [ /PDFX1a:2001 ] >>").standard.as_deref(), Some("x1a"));
}

#[test]
fn a_partial_file_changes_only_what_it_names() {
    let p = preset("Tiny", "<< /UseDocumentBleed true >>");
    assert_eq!(p.bleed, Some(true));
    assert_eq!(p.standard, None);
    assert_eq!(p.compress_images, None);
    assert_eq!(p.marks_crop, None);
    assert_eq!(p.marks_offset, None);
    assert_eq!(p.tagged, None);
    assert_eq!(p.flatten, None);
    assert!(p.not_applied.is_empty());
}

#[test]
fn registration_marks_and_colour_bars_are_reported() {
    let p = preset("Marks", "<< /AddRegMarks true /AddColorBars true >>");
    assert_eq!(p.not_applied, vec!["Registration marks are not supported.", "Colour bars are not supported."]);
}

#[test]
fn flattening_follows_the_pdf_version_not_allow_transparency() {
    // Adobe's own presets say `/AllowTransparency false` at every version; InDesign flattens only for PDF 1.3.
    assert_eq!(preset("a", "<< /CompatibilityLevel 1.3 >>").flatten.as_deref(), Some("high"));
    assert_eq!(preset("b", "<< /CompatibilityLevel 1.4 /AllowTransparency false >>").flatten.as_deref(), Some(""));
    assert_eq!(preset("c", "<< /CompatibilityLevel 1.7 >>").flatten.as_deref(), Some(""));
    assert_eq!(preset("d", "<< /AllowTransparency false >>").flatten, None);
}

#[test]
fn hex_strings_are_read_as_utf16_text() {
    let jo = parse("<< /Description << /ARA <FEFF0627 0628> /ENU (Hi) /DEU <fEFF\n00410042> >> /A true >>").unwrap();
    assert_eq!(jo.text("ARA"), Some("اب"));
    assert_eq!(jo.text("DEU"), Some("AB"));
    assert_eq!(jo.text("ENU"), Some("Hi"));
    assert_eq!(jo.flag("A"), Some(true), "the key after the hex strings is still read");
}

#[test]
fn a_cut_off_hex_string_is_malformed() {
    assert!(matches!(parse("<< /A <FEFF00"), Err(JobOptionsError::Malformed(_))));
}

#[test]
fn pdf_x4_and_pdf_a_compliance_names_map_to_their_standards() {
    let x4 = preset("x4", "<< /CheckCompliance [ /PDFX4:2008 ] >>");
    assert_eq!(x4.standard.as_deref(), Some("x4"));
    assert!(x4.not_applied.is_empty(), "{:?}", x4.not_applied);
    let a = preset("a", "<< /CheckCompliance [ /PDFA1B:2005 ] >>");
    assert_eq!(a.standard.as_deref(), Some("a2b"));
    assert_eq!(a.not_applied, vec!["PDF/A-1b was asked for. DesignCraft writes PDF/A-2b."]);
    let a2 = preset("a2", "<< /CheckCompliance [ /PDFA2B:2011 ] >>");
    assert_eq!(a2.standard.as_deref(), Some("a2b"));
    assert!(a2.not_applied.is_empty(), "{:?}", a2.not_applied);
}

#[test]
fn a_preset_survives_a_json_round_trip() {
    let p = preset("Blurb", BLURB);
    let json = serde_json::to_string(&p).unwrap();
    assert!(json.contains("\"compressImages\":true"), "{json}");
    let back: ImportedPreset = serde_json::from_str(&json).unwrap();
    assert_eq!(back, p);
}

#[test]
fn image_encoding_off_means_images_are_not_compressed() {
    let off = preset("off", "<< /EncodeColorImages false /ColorImageFilter /DCTEncode >>");
    assert_eq!(off.compress_images, Some(false));
    let auto = preset("auto", "<< /AutoFilterColorImages true /ColorImageFilter /FlateEncode >>");
    assert_eq!(auto.compress_images, Some(true));
}

#[test]
fn marks_numbers_outside_a_sane_range_are_left_out_and_reported() {
    let ok = preset("ok", "<< /MarksWeight 0.25 /MarksOffset 6 >>");
    assert_eq!((ok.marks_weight.as_deref(), ok.marks_offset.as_deref()), (Some("0.25"), Some("6")));
    assert!(ok.not_applied.is_empty(), "{:?}", ok.not_applied);
    for bad in ["1e999", "99999", "-3"] {
        let p = preset("bad", &format!("<< /MarksWeight {bad} /MarksOffset {bad} >>"));
        assert_eq!((p.marks_weight, p.marks_offset), (None, None), "{bad}");
        assert_eq!(p.not_applied.len(), 2, "{bad}: {:?}", p.not_applied);
    }
}

#[test]
fn an_empty_or_none_output_intent_profile_gives_no_note() {
    for profile in ["()", "(None)", "(none)"] {
        let p = preset("x", &format!("<< /PDFX1aCheck true /PDFXOutputIntentProfile {profile} >>"));
        assert!(p.not_applied.is_empty(), "{profile}: {:?}", p.not_applied);
    }
}

#[test]
fn utf16_files_are_read_with_or_without_the_byte_order_mark() {
    let text = "<< /AddCropMarks true >>";
    let le: Vec<u8> = [0xFF, 0xFE].into_iter().chain(text.encode_utf16().flat_map(u16::to_le_bytes)).collect();
    let be: Vec<u8> = [0xFE, 0xFF].into_iter().chain(text.encode_utf16().flat_map(u16::to_be_bytes)).collect();
    for bytes in [le, be, text.as_bytes().to_vec()] {
        assert_eq!(parse_bytes(&bytes).unwrap().flag("AddCropMarks"), Some(true));
    }
}
