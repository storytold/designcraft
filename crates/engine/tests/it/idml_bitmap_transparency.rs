//! IDML preserves the Black swatch on bitmap artwork so its white pixels stay transparent.

use std::sync::Arc;

use designcraft_color::{Color, Swatch};
use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Asset, AssetId, Content, Document, Fill, Graphic, Item, ItemId, Shape, SpreadRef};
use designcraft_geom::{Affine, Rect, shapes};
use designcraft_render::{RenderOptions, Renderer};

const CYAN: &str = "Bitmap test cyan";

/// A synthetic, one-row 8-bit-width TIFF bitmap: even pixels are white and odd pixels are black.
/// The sample pixels and TIFF bytes are generated here, with no external artwork or fixture file.
fn one_bit_tiff() -> Vec<u8> {
    // Header and IFD: width, height, one bit per sample, no compression, BlackIsZero,
    // one strip, one row per strip and one byte of pixel samples.
    let mut bytes = b"II*\0\x08\0\0\0\x08\0".to_vec();
    let entries: [(u16, u16, u32); 8] = [(256, 3, 8), (257, 3, 1), (258, 3, 1), (259, 3, 1), (262, 3, 1), (273, 4, 110), (278, 3, 1), (279, 4, 1)];
    for (tag, kind, value) in entries {
        bytes.extend_from_slice(&tag.to_le_bytes());
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        if kind == 3 {
            bytes.extend_from_slice(&(value as u16).to_le_bytes());
            bytes.extend_from_slice(&[0, 0]);
        } else {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0b1010_1010);
    bytes
}

fn imported_bitmap_over_cyan() -> Document {
    let mut doc = Document::new(&NewDocument { width: 80.0, height: 20.0, facing_pages: false, ..Default::default() });
    doc.swatches.push(Swatch::color(CYAN, Color::rgb(0.0, 1.0, 1.0)));
    let layer = doc.default_layer();
    let bounds = Rect::new(0.0, 0.0, 80.0, 20.0);

    let asset = AssetId(doc.alloc());
    doc.assets.insert(
        asset,
        Arc::new(Asset {
            page: 0,
            id: asset,
            name: "bitmap.tif".into(),
            mime: "image/tiff".into(),
            data: designcraft_doc::AssetData::embedded(one_bit_tiff()),
            pixels: Some((8, 1)),
            ..Default::default()
        }),
    );
    let mut image = Item::new(ItemId(doc.alloc()), layer, Shape::Rectangle, shapes::rectangle(bounds));
    image.fill = Fill::swatch(CYAN);
    image.content = Content::Graphic(Graphic {
        layer_options: None,
        asset,
        size: (80.0, 20.0),
        xf: Affine::IDENTITY,
        clip: None,
        auto_fit: Default::default(),
        fit_align: 4,
        crop: [0.0; 4],
        wrap: Default::default(),
        fill: Fill::swatch("[Black]"),
    });
    doc.insert_item(SpreadRef::Doc(0), image, None).unwrap();

    let idml = designcraft_idml::export_idml(&doc);
    designcraft_idml::import_idml(&idml).unwrap()
}

#[test]
fn imported_bitmap_white_reveals_the_frame_while_black_stays_visible() {
    let document = imported_bitmap_over_cyan();
    let mut renderer = Renderer::new();
    renderer.threads = 0;
    let rendered = renderer.render_page(&document, &Cache::new(), 0, 1.0, false, &RenderOptions::default()).unwrap();

    let white_pixel = rendered.pixel(5, 10);
    let black_pixel = rendered.pixel(15, 10);
    assert!(white_pixel[1] > 240 && white_pixel[2] > 240 && white_pixel[0] < 20, "white bitmap pixels reveal the cyan frame: {white_pixel:?}");
    assert!(black_pixel[0] < 20 && black_pixel[1] < 20 && black_pixel[2] < 20, "black bitmap pixels remain visible: {black_pixel:?}");
}
