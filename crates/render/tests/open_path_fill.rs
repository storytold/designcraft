//! A path with an open contour is filled as if it were closed, as InDesign does.

use designcraft_color::{Color, Swatch};
use designcraft_compose::Cache;
use designcraft_doc::{Document, Fill, Item, ItemId, Shape, SpreadRef, build::NewDocument};
use designcraft_geom::{Affine, Point, SubPath, path::PathData};
use designcraft_render::{Placed, RenderOptions, Renderer};

#[test]
fn an_open_contour_is_filled() {
    let mut d = Document::new(&NewDocument { width: 100.0, height: 100.0, facing_pages: false, ..Default::default() });
    d.swatches.push(Swatch::color("Red", Color::rgb(1.0, 0.0, 0.0)));
    let id = ItemId(d.alloc());
    // A triangle whose contour is open: it still has an inside.
    let tri = SubPath::polyline(&[Point::new(10.0, 10.0), Point::new(90.0, 10.0), Point::new(50.0, 90.0)], false);
    let mut it = Item::new(id, d.default_layer(), Shape::Path, PathData::new(vec![tri]));
    it.fill = Fill::swatch("Red");
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    let mut r = Renderer::new();
    r.threads = 0;
    let placed = [Placed { spread: SpreadRef::Doc(0), xf: Affine::IDENTITY }];
    let img = r.render(&d, &Cache::new(), &placed, 100, 100, Affine::IDENTITY, &RenderOptions::default());
    let i = ((30 * 100 + 50) * 4) as usize;
    assert_eq!(img.pixels[i..i + 3], [255, 0, 0], "the inside of an open triangle is red");
}
