//! A placed PCX (ZSoft Paintbrush) raster draws its pixels — not a black frame — for the
//! paletted and true-colour forms old software writes.

use std::sync::Arc;

use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Asset, AssetId, Content, Document, Fill, Graphic, Item, ItemId, Shape, SpreadRef, Stroke};
use designcraft_geom::{Affine, Rect, shapes};
use designcraft_render::{RenderOptions, Renderer};

/// A PCX header for a `w`×`h` image at `bits` per plane with `planes` planes.
fn header(bits: u8, planes: u8, w: u16, h: u16, bytes_per_line: u16) -> Vec<u8> {
    let mut b = vec![0u8; 128];
    b[0] = 0x0A;
    b[1] = 5;
    b[2] = 1;
    b[3] = bits;
    b[8..10].copy_from_slice(&(w - 1).to_le_bytes());
    b[10..12].copy_from_slice(&(h - 1).to_le_bytes());
    b[65] = planes;
    b[66..68].copy_from_slice(&bytes_per_line.to_le_bytes());
    b
}

/// 3×1, 8 bits, three planes: red, green, blue (line padding to four bytes).
fn rgb_pcx() -> Vec<u8> {
    let mut b = header(8, 3, 3, 1, 4);
    b.extend([180, 0, 0, 0, 0, 181, 0, 0, 0, 0, 182, 0]);
    b
}

/// 3×1, 8 bits, one plane with a trailing 256-colour palette: red, green, blue.
fn paletted_pcx() -> Vec<u8> {
    let mut b = header(8, 1, 3, 1, 4);
    b.extend([1, 2, 3, 0]);
    b.push(0x0C);
    let mut palette = [[0u8; 3]; 256];
    palette[1] = [200, 0, 0];
    palette[2] = [0, 201, 0];
    palette[3] = [0, 0, 202];
    for colour in palette {
        b.extend(colour);
    }
    b
}

/// The placed image's colours at one third, one half and two thirds across, on a white page.
fn shown(file: Vec<u8>) -> Vec<[u8; 4]> {
    let mut d = Document::new(&NewDocument::default());
    let aid = AssetId(d.alloc());
    d.assets.insert(
        aid,
        Arc::new(Asset {
            page: 0,
            id: aid,
            name: "art.pcx".into(),
            mime: "image/pcx".into(),
            link: None,
            data: Arc::new(file),
            pixels: Some((3, 1)),
            pdf_crop: Default::default(),
            pdf_box: None,
        }),
    );
    let id = ItemId(d.alloc());
    let mut it = Item::new(id, d.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(100.0, 100.0, 250.0, 150.0)));
    it.content = Content::Graphic(Graphic {
        layer_options: None,
        asset: aid,
        size: (3.0, 1.0),
        xf: Affine::scale_non_uniform(50.0, 50.0) * Affine::translate((2.0, 2.0)),
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
    [125, 175, 225].iter().map(|&x| img.pixel(x, 125)).collect()
}

fn near(a: [u8; 4], b: [u8; 3]) -> bool {
    a[3] == 255 && a[..3].iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 4)
}

/// A 2×1 one-bit bitmap: black ink, white paper (`0x40` is black then white).
fn one_bit_pcx() -> Vec<u8> {
    let mut b = header(1, 1, 2, 1, 1);
    b[19..22].copy_from_slice(&[255, 255, 255]);
    b.push(0x40);
    b
}

/// The two image pixels and a bare patch of the red backdrop behind them.
fn shown_on_red(file: Vec<u8>) -> ([u8; 4], [u8; 4], [u8; 4]) {
    const RED: &str = "C=0 M=100 Y=100 K=0";
    let mut d = Document::new(&NewDocument::default());
    let aid = AssetId(d.alloc());
    d.assets.insert(
        aid,
        Arc::new(Asset {
            page: 0,
            id: aid,
            name: "art.pcx".into(),
            mime: "image/pcx".into(),
            link: None,
            data: Arc::new(file),
            pixels: Some((2, 1)),
            pdf_crop: Default::default(),
            pdf_box: None,
        }),
    );
    let backdrop = ItemId(d.alloc());
    let mut back = Item::new(backdrop, d.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(100.0, 100.0, 350.0, 150.0)));
    back.fill = Fill::swatch(RED);
    back.stroke = Stroke::default();
    d.insert_item(SpreadRef::Doc(0), back, None).unwrap();
    let id = ItemId(d.alloc());
    let mut it = Item::new(id, d.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(100.0, 100.0, 250.0, 150.0)));
    it.fill = Fill::none();
    it.content = Content::Graphic(Graphic {
        layer_options: None,
        asset: aid,
        size: (2.0, 1.0),
        xf: Affine::translate((100.0, 100.0)) * Affine::scale_non_uniform(75.0, 50.0),
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
    (img.pixel(137, 125), img.pixel(212, 125), img.pixel(300, 125))
}

#[test]
fn a_placed_one_bit_pcx_lets_the_page_show_through_white() {
    let (ink, paper, backdrop) = shown_on_red(one_bit_pcx());
    assert!(near(ink, [0, 0, 0]), "black pixels are ink: {ink:?}");
    assert!(
        paper[..3].iter().zip(&backdrop[..3]).all(|(a, b)| a.abs_diff(*b) <= 4),
        "white pixels are transparent, not a white box: {paper:?} over {backdrop:?}"
    );
    assert!(!near(backdrop, [255, 255, 255]), "the backdrop is not white: {backdrop:?}");
}

#[test]
fn a_placed_true_colour_pcx_draws_its_planes() {
    let got = shown(rgb_pcx());
    assert!(near(got[0], [180, 0, 0]), "red plane: {got:?}");
    assert!(near(got[1], [0, 181, 0]), "green plane: {got:?}");
    assert!(near(got[2], [0, 0, 182]), "blue plane: {got:?}");
}

#[test]
fn a_placed_paletted_pcx_draws_its_trailing_palette() {
    let got = shown(paletted_pcx());
    assert!(near(got[0], [200, 0, 0]), "index 1: {got:?}");
    assert!(near(got[1], [0, 201, 0]), "index 2: {got:?}");
    assert!(near(got[2], [0, 0, 202]), "index 3: {got:?}");
}
