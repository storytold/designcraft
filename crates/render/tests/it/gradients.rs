//! Gradient fills: InDesign stretches a radial gradient with the object's box.

use designcraft_color::swatch::{Swatch, SwatchValue};
use designcraft_color::{Color, Gradient, GradientKind, GradientStop};
use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Document, Fill, Item, ItemId, Shape, SpreadRef};
use designcraft_geom::{Affine, Rect, Vec2, shapes};
use designcraft_render::{Placed, RenderOptions, Rendered, Renderer};

/// A 100×200 rectangle at (50, 20) filled with a black-to-white radial gradient.
fn portrait(vector: Option<[f64; 4]>) -> Document {
    let mut d = Document::new(&NewDocument { width: 200.0, height: 240.0, facing_pages: false, ..Default::default() });
    let stop = |offset: f32, color: Color| GradientStop::new(offset, color);
    d.swatches.push(Swatch {
        name: "Radial".into(),
        value: SwatchValue::Gradient {
            gradient: Gradient { kind: GradientKind::Radial, stops: vec![stop(0.0, Color::BLACK), stop(1.0, Color::WHITE)] },
        },
        locked: false,
        named: true,
        hidden: false,
    });
    let id = ItemId(d.alloc());
    let mut it = Item::new(id, d.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(50.0, 20.0, 150.0, 220.0)));
    it.fill = Fill { swatch: "Radial".into(), gradient_vector: vector, ..Fill::none() };
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    d
}

fn render(d: &Document) -> Rendered {
    let mut r = Renderer::new();
    r.threads = 0;
    let opts = RenderOptions { background: Some([255, 255, 255, 255]), ..Default::default() };
    r.render(d, &Cache::new(), &[Placed { spread: SpreadRef::Doc(0), xf: Affine::translate(Vec2::ZERO) }], 200, 240, Affine::IDENTITY, &opts)
}

fn grey(img: &Rendered, x: u32, y: u32) -> u8 {
    img.pixel(x, y)[0]
}

#[test]
fn a_radial_gradient_stretches_with_its_object() {
    // Centred, it reaches white at the sides and at the top and bottom alike (an ellipse).
    let img = render(&portrait(None));
    assert!(grey(&img, 100, 120) < 20, "black at the centre: {}", grey(&img, 100, 120));
    let (side, top) = (grey(&img, 52, 120), grey(&img, 100, 22));
    assert!(side > 225 && top > 225, "white at the side ({side}) and the top ({top})");
    // Half-way to the side and half-way to the top: the same grey.
    let (hs, ht) = (grey(&img, 75, 120), grey(&img, 100, 70));
    assert!(hs.abs_diff(ht) < 12, "half-way: side {hs}, top {ht}");
    // A Gradient Swatch tool vector to the right side: the same ellipse.
    let dragged = render(&portrait(Some([100.0, 120.0, 150.0, 120.0])));
    assert!(grey(&dragged, 100, 70).abs_diff(ht) < 12 && grey(&dragged, 75, 120).abs_diff(hs) < 12);
}
