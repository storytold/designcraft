//! A placed WMF (Windows Metafile) draws as a vector with a transparent background: the page
//! shows through the parts the drawing does not paint, as in InDesign.

use std::sync::Arc;

use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Asset, AssetId, Content, Document, Fill, Graphic, Item, ItemId, Shape, SpreadRef, Stroke};
use designcraft_geom::{Affine, Rect, shapes};
use designcraft_render::{RenderOptions, Renderer};

/// A WMF record: four-byte size in words, function, then word-aligned parameters.
fn record(function: u16, params: &[u8]) -> Vec<u8> {
    let words = (6 + params.len()).div_ceil(2);
    let mut r = Vec::new();
    r.extend(u32::try_from(words).unwrap().to_le_bytes());
    r.extend(function.to_le_bytes());
    r.extend(params);
    if params.len() % 2 == 1 {
        r.push(0);
    }
    r
}

/// A placeable WMF `w`×`h` at 96 units per inch holding `body`.
fn wmf(w: i16, h: i16, body: &[Vec<u8>]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(0x9AC6_CDD7u32.to_le_bytes());
    b.extend(0u16.to_le_bytes());
    for v in [0, 0, w, h] {
        b.extend(v.to_le_bytes());
    }
    b.extend(96u16.to_le_bytes());
    b.extend(0u32.to_le_bytes());
    b.extend(0u16.to_le_bytes());
    let body: Vec<u8> = body.concat();
    let words = (22 + 18 + body.len()) / 2;
    b.extend(1u16.to_le_bytes());
    b.extend(9u16.to_le_bytes());
    b.extend(0x0300u16.to_le_bytes());
    b.extend(u32::try_from(words).unwrap().to_le_bytes());
    b.extend(0u16.to_le_bytes());
    b.extend(0u32.to_le_bytes());
    b.extend(0u16.to_le_bytes());
    b.extend(body);
    b
}

/// A solid brush object.
fn brush(colour: [u8; 3]) -> Vec<u8> {
    let reference = u32::from(colour[0]) | u32::from(colour[1]) << 8 | u32::from(colour[2]) << 16;
    let mut p = Vec::new();
    p.extend(0u16.to_le_bytes());
    p.extend(reference.to_le_bytes());
    p.extend(0u16.to_le_bytes());
    record(0x02FC, &p)
}

fn select(index: u16) -> Vec<u8> {
    record(0x012D, &index.to_le_bytes())
}

/// A `META_RECTANGLE` in logical units; WMF stores bottom, right, top, left.
fn rectangle(left: i16, top: i16, right: i16, bottom: i16) -> Vec<u8> {
    let mut p = Vec::new();
    for v in [bottom, right, top, left] {
        p.extend(v.to_le_bytes());
    }
    record(0x041B, &p)
}

/// A WMF that paints its lower half blue and leaves the upper half unpainted.
fn half_blue() -> Vec<u8> {
    wmf(100, 100, &[brush([0, 128, 255]), select(0), rectangle(0, 50, 100, 100), record(0, &[])])
}

fn near(a: [u8; 4], b: [u8; 4]) -> bool {
    a[3] == b[3] && a[..3].iter().zip(&b[..3]).all(|(x, y)| x.abs_diff(*y) <= 4)
}

fn shown(file: Vec<u8>) -> ([u8; 4], [u8; 4], [u8; 4]) {
    const RED: &str = "C=0 M=100 Y=100 K=0";
    let mut d = Document::new(&NewDocument::default());
    let aid = AssetId(d.alloc());
    d.assets.insert(
        aid,
        Arc::new(Asset {
            page: 0,
            id: aid,
            name: "art.wmf".into(),
            mime: "image/x-wmf".into(),
            link: None,
            data: Arc::new(file),
            pixels: Some((72, 72)),
            pdf_crop: Default::default(),
            pdf_box: None,
        }),
    );
    let backdrop = ItemId(d.alloc());
    let mut back = Item::new(backdrop, d.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(50.0, 50.0, 300.0, 300.0)));
    back.fill = Fill::swatch(RED);
    back.stroke = Stroke::default();
    d.insert_item(SpreadRef::Doc(0), back, None).unwrap();
    let id = ItemId(d.alloc());
    let mut it = Item::new(id, d.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(100.0, 100.0, 172.0, 172.0)));
    it.fill = Fill::none();
    it.content = Content::Graphic(Graphic {
        layer_options: None,
        asset: aid,
        size: (72.0, 72.0),
        xf: Affine::translate((100.0, 100.0)),
        clip: None,
        auto_fit: Default::default(),
        fit_align: 4,
        crop: [0.0; 4],
        wrap: Default::default(),
        fill: Default::default(),
    });
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    let mut r = Renderer::new();
    r.threads = 0;
    let img = r.render_page(&d, &Cache::new(), 0, 1.0, false, &RenderOptions::default()).unwrap();
    (img.pixel(136, 118), img.pixel(136, 154), img.pixel(250, 250))
}

#[test]
fn a_placed_wmf_draws_its_shapes_over_a_transparent_background() {
    let (top, bottom, backdrop) = shown(half_blue());
    assert!(near(top, backdrop), "the unpainted half shows the page: {top:?} vs {backdrop:?}");
    assert!(near(bottom, [0, 128, 255, 255]), "the blue rectangle: {bottom:?}");
    assert!(!near(backdrop, [255, 255, 255, 255]), "the page is not white: {backdrop:?}");
}

#[test]
fn an_unsupported_wmf_shows_an_empty_frame_rather_than_partial_art() {
    // META_TEXTOUT (0x0521): GDI text can't be replayed, so the file is refused as a whole.
    let unsupported = wmf(100, 100, &[record(0x0521, &[0; 8]), record(0, &[])]);
    assert!(designcraft_images::is_wmf(&unsupported));
    assert_eq!(designcraft_images::decode_rgba(&unsupported), None);
    assert_eq!(designcraft_images::pixel_size(&unsupported), None);
}
