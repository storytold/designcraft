//! A swatch applied to a placed greyscale or bitmap image colours it, as in InDesign (#1094):
//! a greyscale pixel is a tint of the swatch as strong as the pixel is dark, and white stays
//! opaque; a bitmap's black takes the swatch and its white lets the frame's fill show through.
//! Colour images, and swatches that no longer exist, leave the image as it is.

use std::sync::Arc;

use designcraft_color::swatch::apply_tint;
use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Asset, AssetId, Content, Document, Fill, Graphic, Item, ItemId, Shape, SpreadRef, Stroke};
use designcraft_geom::{Affine, Rect, shapes};
use designcraft_render::{RenderOptions, Renderer, color_of};
use image::{ExtendedColorType, ImageEncoder};

const RED: &str = "C=0 M=100 Y=100 K=0";
const CYAN: &str = "C=100 M=0 Y=0 K=0";

fn encode(px: &[u8], w: u32, ct: ExtendedColorType, gif: bool) -> Vec<u8> {
    let mut b = Vec::new();
    let c = std::io::Cursor::new(&mut b);
    if gif {
        image::codecs::gif::GifEncoder::new(c).encode(px, w, 1, ct).unwrap();
    } else {
        image::codecs::png::PngEncoder::new(c).write_image(px, w, 1, ct).unwrap();
    }
    b
}

/// The pixels at the centres of the image's three columns, placed 150 × 50 pt at (100, 100) in a
/// cyan frame, with `fill` applied to the image.
fn shown(file: Vec<u8>, fill: Fill) -> Vec<[u8; 4]> {
    let mut d = Document::new(&NewDocument::default());
    let aid = AssetId(d.alloc());
    d.assets.insert(
        aid,
        Arc::new(Asset {
            page: 0,
            id: aid,
            name: "a".into(),
            mime: "image/png".into(),
            link: None,
            data: Arc::new(file),
            pixels: Some((3, 1)),
            pdf_crop: Default::default(),
            pdf_box: None,
        }),
    );
    let id = ItemId(d.alloc());
    let mut it = Item::new(id, d.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(100.0, 100.0, 250.0, 150.0)));
    it.fill = Fill::swatch(CYAN);
    it.stroke = Stroke::default();
    it.content = Content::Graphic(Graphic {
        layer_options: None,
        asset: aid,
        size: (150.0, 50.0),
        xf: Affine::translate((100.0, 100.0)),
        clip: None,
        auto_fit: Default::default(),
        fit_align: 4,
        crop: [0.0; 4],
        wrap: Default::default(),
        fill,
    });
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    let mut r = Renderer::new();
    r.threads = 0;
    let img = r.render_page(&d, &Cache::new(), 0, 1.0, false, &RenderOptions::default()).unwrap();
    [125, 175, 225].iter().map(|&x| img.pixel(x, 125)).collect()
}

/// The swatch `name` at `tint` on screen.
fn swatch_rgb(name: &str, tint: f32) -> [u8; 4] {
    let d = Document::new(&NewDocument::default());
    let [r, g, b, a] = color_of(&apply_tint(d.resolve_color(name, 1.0).unwrap(), tint), 1.0).to_rgba8().to_u8_array();
    [r, g, b, a]
}

fn near(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 4)
}

#[test]
fn a_greyscale_image_is_a_tint_of_the_swatch_on_opaque_white() {
    let grey = encode(&[0, 128, 255], 3, ExtendedColorType::L8, false);
    let got = shown(grey.clone(), Fill::swatch(RED));
    assert!(near(got[0], swatch_rgb(RED, 1.0)), "black is the swatch: {got:?}");
    assert!(near(got[1], swatch_rgb(RED, 127.0 / 255.0)), "mid grey is half the swatch: {got:?}");
    // White is 0% of the swatch, opaque: the cyan frame does not show through.
    assert!(near(got[2], swatch_rgb(RED, 0.0)), "white stays white: {got:?}");
    assert!(got[2][0] > 240, "{got:?}");

    // The swatch's tint scales every pixel's.
    let half = shown(grey, Fill { tint: 0.5, ..Fill::swatch(RED) });
    assert!(near(half[0], swatch_rgb(RED, 0.5)), "{half:?}");
}

#[test]
fn a_bitmap_image_shows_the_frame_through_its_white() {
    let bw = [[0u8, 0, 0, 255], [255, 255, 255, 255], [0, 0, 0, 255]].concat();
    let got = shown(encode(&bw, 3, ExtendedColorType::Rgba8, true), Fill::swatch(RED));
    assert!(near(got[0], swatch_rgb(RED, 1.0)) && near(got[2], got[0]), "{got:?}");
    assert!(near(got[1], swatch_rgb(CYAN, 1.0)), "the frame's cyan through the white: {got:?}");
}

#[test]
fn colour_images_and_missing_swatches_are_left_alone() {
    let rgb = encode(&[0, 0, 0, 200, 40, 40, 255, 255, 255], 3, ExtendedColorType::Rgb8, false);
    assert_eq!(shown(rgb.clone(), Fill::swatch(RED)), shown(rgb, Fill::none()));
    // A swatch that was deleted (or never imported): the image as it is.
    let grey = encode(&[0, 128, 255], 3, ExtendedColorType::L8, false);
    let plain = shown(grey.clone(), Fill::none());
    assert_eq!(shown(grey, Fill::swatch("Gone")), plain);
    assert!(plain[0][0] < 10, "the image is drawn: {plain:?}");
}
