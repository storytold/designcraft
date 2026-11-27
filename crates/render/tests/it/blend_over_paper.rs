//! Blend modes blend with the objects below, never with the paper: over blank paper an object in
//! Color Dodge (or any other mode) shows as it is, as in InDesign and in the exported PDF.

use designcraft_color::{BlendMode, Color, Swatch};
use designcraft_compose::Cache;
use designcraft_doc::{Document, Fill, Item, ItemId, Shape, SpreadRef, build::NewDocument};
use designcraft_geom::{Affine, Rect, shapes};
use designcraft_render::{Placed, RenderOptions, Renderer};

/// A 200 x 100 page: a mid-blue rectangle on the right half, and over both halves a black
/// rectangle in `blend`.
fn document(blend: BlendMode) -> Document {
    let mut d = Document::new(&NewDocument { width: 200.0, height: 100.0, facing_pages: false, ..Default::default() });
    d.swatches.push(Swatch::color("Mid blue", Color::rgb(0.2, 0.4, 0.6)));
    let mut rect = |r: Rect, swatch: &str, blend: BlendMode| {
        let id = ItemId(d.alloc());
        let mut it = Item::new(id, d.default_layer(), Shape::Rectangle, shapes::rectangle(r));
        it.fill = Fill::swatch(swatch);
        it.blend = blend;
        d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    };
    rect(Rect::new(100.0, 0.0, 200.0, 100.0), "Mid blue", BlendMode::Normal);
    rect(Rect::new(20.0, 20.0, 180.0, 80.0), "[Black]", blend);
    d
}

fn pixel(d: &Document, x: u32, y: u32) -> [u8; 4] {
    let mut r = Renderer::new();
    r.threads = 0;
    let placed = [Placed { spread: SpreadRef::Doc(0), xf: Affine::IDENTITY }];
    let img = r.render(d, &Cache::new(), &placed, (200, 100), Affine::IDENTITY, &RenderOptions::default());
    let i = ((y * 200 + x) * 4) as usize;
    [img.pixels[i], img.pixels[i + 1], img.pixels[i + 2], img.pixels[i + 3]]
}

#[test]
fn a_blend_mode_over_blank_paper_shows_the_object_as_it_is() {
    let d = document(BlendMode::ColorDodge);
    // Over the paper: black ([Black] shows as about 52 grey), as in Normal (it used to dodge the white paper to white, vanishing).
    let over_paper = pixel(&d, 50, 50);
    assert!(over_paper[0] < 80 && over_paper[1] < 80 && over_paper[2] < 80, "{over_paper:?}");
    // Over the blue rectangle it still blends: black in Color Dodge leaves the blue as it is.
    let over_blue = pixel(&d, 150, 50);
    assert!(over_blue[2] > 120 && over_blue[2] > over_blue[0], "{over_blue:?}");
    // Normal is unchanged.
    let normal = pixel(&document(BlendMode::Normal), 150, 50);
    assert!(normal[0] < 80 && normal[2] < 80, "{normal:?}");
}
