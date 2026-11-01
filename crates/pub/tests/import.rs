// Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0
//! Publisher import on a synthetic publication (`tests/data/hello.pub`, written from scratch by
//! `support::hello`: one text box and one filled rectangle on a US Letter page), plus hostile
//! input.

mod support;

use std::io::{Cursor, Read, Write};

use designcraft_doc::{Content, Document, Item};
use designcraft_pub::{PubError, import_pub, is_pub};

const HELLO: &[u8] = include_bytes!("data/hello.pub");

#[test]
#[ignore = "requires PUB_CORPUS with the allowed pubsmith binary fixtures"]
fn real_shape_shadows() {
    let dirs = std::env::var("PUB_CORPUS").unwrap();
    for (name, offset, color) in
        [("shadow__on.pub", 2.0, [0, 0, 0, 255]), ("shadow__offset.pub", 6.0, [0, 0, 0, 255]), ("wordart__shadow.pub", 2.0, [134, 134, 134, 255])]
    {
        let fixture = dirs.split(':').map(|p| std::path::Path::new(p).join(name)).find(|p| p.is_file()).unwrap();
        let imported = import_pub(&std::fs::read(fixture).unwrap()).unwrap();
        assert!(!imported.warnings.iter().any(|w| w.contains("shadow")), "{name}: {:?}", imported.warnings);
        let d = &imported.document;
        let shadows: Vec<_> = items(d).into_iter().filter(|i| i.effects.drop_shadow.on).collect();
        assert_eq!(shadows.len(), 1, "{name}");
        let item = shadows[0];
        let ds = &item.effects.drop_shadow;
        assert_eq!(ds.opacity, 1.0, "{name}");
        assert_eq!((ds.size, ds.spread), (0.0, 0.0));
        assert!((-ds.angle.to_radians().cos() * ds.distance - offset).abs() < 1e-9, "{name}");
        assert!((ds.angle.to_radians().sin() * ds.distance - offset).abs() < 1e-9, "{name}");
        let designcraft_color::SwatchValue::Color { color: actual, .. } = &d.swatch(&ds.color).unwrap().value else { panic!("not a color") };
        assert_eq!(actual.to_rgba8(1.0), color, "{name}");
        assert_eq!(item.opacity, 1.0);
    }
}

#[test]
#[ignore = "requires PUB_CORPUS with the allowed pubsmith binary fixtures"]
fn real_solid_fill_opacity() {
    let dirs = std::env::var("PUB_CORPUS").unwrap();
    let fixture = dirs.split(':').map(|p| std::path::Path::new(p).join("fill__transparency-50.pub")).find(|p| p.is_file()).unwrap();
    let imported = import_pub(&std::fs::read(fixture).unwrap()).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let d = &imported.document;
    let transparent: Vec<_> = items(d)
        .into_iter()
        .filter_map(|item| designcraft_color::swatch::resolve_gradient(&d.swatches, &item.fill.swatch).map(|g| (item, g)))
        .collect();
    assert_eq!(transparent.len(), 1);
    let (item, gradient) = transparent[0];
    assert_eq!(gradient.stops.len(), 2);
    for stop in &gradient.stops {
        assert_eq!(stop.opacity, 0.5);
        assert_eq!(stop.color.to_rgba8(1.0), [0, 0, 255, 255]);
    }
    assert_eq!(item.opacity, 1.0);
    assert_ne!(item.stroke.swatch, designcraft_color::swatch::NONE);
    assert!(item.stroke.weight > 0.0);
}

fn items(d: &Document) -> Vec<&Item> {
    d.spreads.iter().flat_map(|s| s.items.iter().map(|i| &**i)).collect()
}

fn text_of(d: &Document, it: &Item) -> Option<String> {
    let Content::Text(t) = &it.content else { return None };
    d.stories.get(&t.story).map(|s| s.text.clone())
}

/// The fixture with its `Contents` stream replaced by `f(contents)`.
fn with_contents(f: impl FnOnce(Vec<u8>) -> Vec<u8>) -> Vec<u8> {
    let mut cf = cfb::CompoundFile::open(Cursor::new(HELLO.to_vec())).unwrap();
    let mut contents = Vec::new();
    cf.open_stream("/Contents").unwrap().read_to_end(&mut contents).unwrap();
    let new = f(contents);
    let mut s = cf.create_stream("/Contents").unwrap();
    s.write_all(&new).unwrap();
    drop(s);
    cf.flush().unwrap();
    cf.into_inner().into_inner()
}

#[test]
fn imports_text_box_and_rectangle_as_editable_objects() {
    assert!(is_pub(HELLO));
    let imp = import_pub(HELLO).unwrap();
    let d = &imp.document;
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    assert_eq!(d.page_count(), 1);
    let page = &d.spreads[0].pages[0];
    assert_eq!((page.width, page.height), (612.0, 792.0));
    let its = items(d);
    assert_eq!(its.len(), 2);

    let text = its.iter().find(|i| i.is_text_frame()).unwrap();
    assert_eq!(text_of(d, text).as_deref(), Some(support::HELLO_TEXT));
    let b = text.inner_bounds();
    assert!((b.x0 - 72.0).abs() < 0.5 && (b.y0 - 72.0).abs() < 0.5 && (b.width() - 288.0).abs() < 0.5 && (b.height() - 72.0).abs() < 0.5, "{b:?}");

    let rect = its.iter().find(|i| !i.is_text_frame()).unwrap();
    let b = rect.inner_bounds();
    assert!((b.x0 - 72.0).abs() < 0.5 && (b.y0 - 216.0).abs() < 0.5 && (b.width() - 216.0).abs() < 0.5 && (b.height() - 144.0).abs() < 0.5, "{b:?}");
    let sw = d.swatch(&rect.fill.swatch).unwrap();
    let designcraft_color::SwatchValue::Color { color, .. } = &sw.value else { panic!("fill is not a colour: {sw:?}") };
    let [r, g, bl, _] = color.to_rgba8(1.0);
    assert_eq!([r, g, bl], [0x1f, 0x6f, 0xd0]);
}

#[test]
fn rejects_files_that_are_not_publications() {
    assert!(matches!(import_pub(b""), Err(PubError::NotPub(_))));
    assert!(matches!(import_pub(b"%PDF-1.7\n"), Err(PubError::NotPub(_))));
    assert!(!is_pub(b"PK\x03\x04"));
    // A compound file without Publisher's streams (here: Contents with a foreign header).
    let other = with_contents(|_| b"\xd0\xcf\x11\xe0 not publisher".to_vec());
    assert!(matches!(import_pub(&other), Err(PubError::NotPub(_))));
}

#[test]
fn refuses_publisher_2000_and_older_with_a_message() {
    let old = with_contents(|mut c| {
        c[2] = 0x22;
        c
    });
    let Err(PubError::Unsupported(msg)) = import_pub(&old) else { panic!("expected Unsupported") };
    assert!(msg.contains("Publisher 97/98/2000"), "{msg}");
}

#[test]
fn truncated_or_corrupt_input_is_an_error_not_a_panic() {
    for n in [8, 512, 1024, 2048, HELLO.len() / 2, HELLO.len() - 1] {
        let _ = import_pub(&HELLO[..n]);
    }
    let short = with_contents(|c| c[..30.min(c.len())].to_vec());
    assert!(import_pub(&short).is_err());
    // Deterministic byte mutations across the whole file must never panic.
    let mut seed = 0x2545_f491_u32;
    for _ in 0..3000 {
        let mut b = HELLO.to_vec();
        for _ in 0..4 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let at = seed as usize % b.len();
            b[at] = (seed >> 24) as u8;
        }
        let _ = import_pub(&b);
    }
}
