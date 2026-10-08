//! Unit tests on synthetic documents built byte by byte (no InDesign-produced files).

use std::collections::BTreeMap;
use std::io::Read;

use crate::container::{Container, PAGE};
use crate::model::{parse_path, parse_runs, parse_text};
use crate::synthetic::{hierarchy, object, rect_path, sample};
use crate::{InddError, is_indd, to_idml, to_idml_named};

fn unzip(idml: &[u8]) -> BTreeMap<String, String> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(idml)).unwrap();
    let mut out = BTreeMap::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let mut s = String::new();
        f.read_to_string(&mut s).unwrap();
        out.insert(f.name().to_string(), s);
    }
    out
}

// --- tests ---------------------------------------------------------------------------------------

#[test]
fn recognises_documents() {
    let doc = sample().build();
    assert!(is_indd(&doc));
    assert!(!is_indd(b"PK\x03\x04"));
    assert!(matches!(to_idml(b"not an indesign file"), Err(InddError::NotIndd)));
}

#[test]
fn reads_objects_from_records() {
    let doc = sample().build();
    let c = Container::parse(&doc).unwrap();
    assert_eq!(c.major, 15);
    assert_eq!(c.classes.get(&30), Some(&0x6201));
    let o = c.object(1).unwrap();
    assert_eq!(&o[..4], &0x501u32.to_le_bytes());
}

#[test]
fn follows_split_records_and_raw_pages() {
    let mut s = sample();
    let big: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
    s.add(90, 0x129, big.clone());
    s.add(91, 0x129, (0..900u32).map(|i| (i % 13) as u8).collect());
    s.raw.push(90);
    s.split.push(91);
    let doc = s.build();
    let c = Container::parse(&doc).unwrap();
    assert_eq!(c.object(90).unwrap(), big);
    assert_eq!(c.object(91).unwrap(), (0..900u32).map(|i| (i % 13) as u8).collect::<Vec<_>>());
}

#[test]
fn converts_to_idml() {
    let idml = to_idml_named(&sample().build(), "card.indd").unwrap();
    assert_eq!(&idml[30..38], b"mimetype");
    let files = unzip(&idml);
    let design = &files["designmap.xml"];
    assert!(design.contains(r#"Name="card.indd""#) && design.contains("Spreads/Spread_u20.xml") && design.contains("Stories/Story_u50.xml"));
    let spread = &files["Spreads/Spread_u20.xml"];
    assert!(spread.contains(r#"GeometricBounds="0 0 100 200""#), "{spread}");
    assert!(spread.contains(r#"<Rectangle Self="u30""#) && spread.contains(r#"FillColor="Color/u12""#));
    assert!(spread.contains(r#"<TextFrame Self="u40""#) && spread.contains(r#"ParentStory="u50""#));
    let story = &files["Stories/Story_u50.xml"];
    assert!(story.contains("<Content>Hello</Content>") && story.contains("<Content>World</Content>"), "{story}");
    assert_eq!(story.matches("<Br/>").count(), 1, "{story}");
    assert!(story.contains(r#"Justification="CenterAlign""#));
    assert!(story.contains(r#"AppliedCharacterStyle="CharacterStyle/Accent""#) && story.contains(r#"PointSize="18""#));
    assert!(files["Resources/Graphic.xml"].contains(r#"Self="Color/u12" Model="Process" Space="CMYK" ColorValue="0 100 100 0" Name="Red""#));
    assert!(files["Resources/Styles.xml"].contains(r#"<CharacterStyle Self="CharacterStyle/Accent""#));
}

#[test]
fn hostile_input_never_panics() {
    let doc = sample().build();
    // Truncations at and around page boundaries, and single-byte corruptions.
    for cut in (0..doc.len()).step_by(PAGE / 4) {
        let _ = to_idml(&doc[..cut]);
    }
    for i in (0..doc.len()).step_by(97) {
        let mut d = doc.clone();
        d[i] ^= 0xFF;
        let _ = to_idml(&d);
    }
    // Self-referencing and shared groups, and repeated segments, under corruption too.
    for d in [with_group(&[30, 30, 31]).build(), with_group(&[31, 31, 30]).build()] {
        for i in (0..d.len()).step_by(211) {
            let mut d = d.clone();
            d[i] ^= 0xFF;
            let _ = to_idml(&d);
        }
    }
    let mut s = sample();
    s.add(95, 0x9999, vec![7; 3200]);
    s.raw.push(95);
    s.repeat_locations.push((95, 600));
    let _ = to_idml(&s.build());
    // Pointer chains that loop must terminate.
    let mut d = doc.clone();
    d[0x3A8..0x3AC].copy_from_slice(&0u32.to_le_bytes());
    assert!(to_idml(&d).is_err());
}

/// Item 30 (a rectangle in the sample) becomes a group with the given children.
fn with_group(kids: &[u32]) -> crate::synthetic::Synth {
    let mut s = sample();
    s.add(30, 0x401, object(&[hierarchy(21, kids)]));
    s.add(31, 0x6201, object(&[(0x162B, rect_path(0.0, 0.0, 10.0, 10.0))]));
    s
}

#[test]
fn self_referencing_group_is_read_once() {
    // Group 30 names itself twice: before the cycle check this expanded to 3^32 items.
    let idml = to_idml(&with_group(&[30, 30, 31]).build()).unwrap();
    let spread = &unzip(&idml)["Spreads/Spread_u20.xml"];
    assert_eq!(spread.matches(r#"<Group Self="u30""#).count(), 1, "{spread}");
    assert_eq!(spread.matches(r#"<Rectangle Self="u31""#).count(), 1, "{spread}");
}

#[test]
fn shared_children_cannot_expand_exponentially() {
    // Groups 100..131 each name the next one twice: 2^32 leaves unless the walk is bounded.
    let mut s = with_group(&[100, 100]);
    for g in 100..132u32 {
        let next = if g == 131 { 31 } else { g + 1 };
        s.add(g, 0x401, object(&[hierarchy(g.saturating_sub(1), &[next, next])]));
    }
    assert!(matches!(to_idml(&s.build()), Err(InddError::TooLarge(_))));
    // A legitimately shared child is still read (once per use).
    let idml = to_idml(&with_group(&[31, 31]).build()).unwrap();
    assert_eq!(unzip(&idml)["Spreads/Spread_u20.xml"].matches(r#"<Rectangle Self="u31""#).count(), 2);
}

#[test]
fn repeated_segments_hit_the_memory_budget() {
    // One 3000-byte raw page named 12,000 times would assemble 36 MB from a ~420 KB file.
    let mut s = sample();
    s.add(95, 0x9999, vec![7; 3200]);
    s.raw.push(95);
    s.repeat_locations.push((95, 12_000));
    let doc = s.build();
    assert!(doc.len() < 500_000, "{}", doc.len());
    assert!(matches!(to_idml(&doc), Err(InddError::TooLarge(_))));
    // The same object named once converts.
    let mut s = sample();
    s.add(95, 0x9999, vec![7; 3200]);
    s.raw.push(95);
    assert!(to_idml(&s.build()).is_ok());
}

#[test]
fn shared_embedded_file_is_read_once_and_output_is_budgeted() {
    // Two frames placing the same embedded PDF share one copy of it.
    let mut s = with_group(&[31, 31]);
    let mut pdf = b"%PDF-1.4 /MediaBox [0 0 10 10] ".to_vec();
    pdf.resize(3000, b' ');
    s.add(80, 0x129, pdf);
    s.add(81, 0x8C41, object(&[(0x8C92, [b"file:/x.pdf\0".to_vec(), 80u32.to_le_bytes().to_vec()].concat())]));
    s.add(82, 0x8C42, object(&[(0x8C9B, [vec![0; 8], 81u32.to_le_bytes().to_vec()].concat())]));
    let bounds: Vec<u8> = [0.0f64, 0.0, 10.0, 10.0].iter().flat_map(|v| v.to_le_bytes()).collect();
    s.add(83, 0x2501, object(&[(0x8CBC, [vec![0; 8], 82u32.to_le_bytes().to_vec()].concat()), (0x1633, bounds)]));
    s.add(31, 0x6201, object(&[(0x162B, rect_path(0.0, 0.0, 10.0, 10.0)), hierarchy(30, &[83])]));
    let bytes = s.build();
    let c = Container::parse(&bytes).unwrap();
    let doc = crate::model::Builder::new(&c).unwrap().build().unwrap();
    let group = doc.spreads[0].items.iter().find(|i| i.uid == 30).unwrap();
    let data: Vec<_> = group.children.iter().map(|k| k.graphic.as_ref().unwrap().data.clone().unwrap()).collect();
    assert_eq!(data.len(), 2);
    assert!(std::sync::Arc::ptr_eq(&data[0], &data[1]));
    let idml = to_idml(&bytes).unwrap();
    assert_eq!(unzip(&idml)["Spreads/Spread_u20.xml"].matches("<PDF Self=\"u83\"").count(), 2);
    // Each use of the graphic counts against the output budget.
    let tight = crate::writer::write_limited(&doc, crate::Budget::for_input(12_000, 1, 0));
    assert!(matches!(tight, Err(InddError::TooLarge(_))));
}

#[test]
fn decoders_tolerate_short_input() {
    assert!(parse_path(&[1, 0, 0, 0, 5, 0]).is_empty());
    assert!(parse_runs(&[0; 9]).is_empty());
    assert_eq!(parse_text(&[0; 3]), "");
    let p = parse_path(&rect_path(0.0, 0.0, 10.0, 5.0));
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].points.len(), 4);
    assert_eq!(p[0].points[2].anchor, (10.0, 5.0));
}

/// A 2×1 palette TIFF with alpha: index 1 = red (opaque), index 2 = blue (half transparent).
fn palette_tiff(compression: u16, pixels: &[u8]) -> Vec<u8> {
    let mut v = b"II*\0".to_vec();
    v.extend_from_slice(&8u32.to_le_bytes());
    let entries: [(u16, u16, u32, u32); 10] = [
        (256, 3, 1, 2),
        (257, 3, 1, 1),
        (258, 3, 1, 8),
        (259, 3, 1, u32::from(compression)),
        (262, 3, 1, 3),
        (273, 4, 1, 0),
        (277, 3, 1, 2),
        (279, 4, 1, pixels.len() as u32),
        (320, 3, 768, 0),
        (338, 3, 1, 2),
    ];
    let ifd_len = 2 + 12 * entries.len() + 4;
    let map_off = 8 + ifd_len;
    let data_off = map_off + 768 * 2;
    v.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, typ, count, value) in entries {
        let value = match tag {
            273 => data_off as u32,
            320 => map_off as u32,
            _ => value,
        };
        v.extend_from_slice(&tag.to_le_bytes());
        v.extend_from_slice(&typ.to_le_bytes());
        v.extend_from_slice(&count.to_le_bytes());
        v.extend_from_slice(&value.to_le_bytes());
    }
    v.extend_from_slice(&0u32.to_le_bytes());
    let mut map = vec![0u16; 768];
    map[1] = 0xFFFF; // red of index 1
    map[512 + 2] = 0xFFFF; // blue of index 2
    for m in map {
        v.extend_from_slice(&m.to_le_bytes());
    }
    v.extend_from_slice(pixels);
    v
}

#[test]
fn palette_tiffs_become_png() {
    for (compression, pixels) in [(1u16, vec![1, 255, 2, 128]), (32773, vec![3, 1, 255, 2, 128])] {
        let png = crate::tiff::palette_to_png(&palette_tiff(compression, &pixels)).unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(img.dimensions(), (2, 1));
        assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(1, 0).0, [0, 0, 255, 128]);
    }
    // PackBits repeat runs and truncated strips do not panic.
    let _ = crate::tiff::palette_to_png(&palette_tiff(32773, &[0x81, 1]));
    // A header claiming 8192×8192 pixels with four bytes of data is rejected before the image is
    // allocated, and strip offsets near the end of the address space do not overflow.
    let mut huge = palette_tiff(1, &[1, 255, 2, 128]);
    huge[18..20].copy_from_slice(&8192u16.to_le_bytes());
    huge[30..32].copy_from_slice(&8192u16.to_le_bytes());
    assert!(crate::tiff::palette_to_png(&huge).is_none());
    let mut far = palette_tiff(1, &[1, 255, 2, 128]);
    let map_entry = 10 + 12 * 8 + 8;
    far[map_entry..map_entry + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(crate::tiff::palette_to_png(&far).is_none());
    let t = palette_tiff(1, &[1, 255, 2, 128]);
    for cut in 0..t.len() {
        let _ = crate::tiff::palette_to_png(&t[..cut]);
    }
}
