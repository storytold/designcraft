//! Synthetic pixel regressions for stroke alignment. No imported artwork is needed.
use designcraft_color::{Color, swatch::Swatch};
use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Document, Fill, Item, ItemId, Join, Shape, SpreadRef, Stroke, StrokeAlign, StrokeType};
use designcraft_geom::{Affine, PathData, Point, Rect, shapes};
use designcraft_render::{Placed, RenderOptions, Rendered, Renderer};

const WHITE: [u8; 4] = [255, 255, 255, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

fn render(path: PathData, fill: bool, stroke: Stroke, xf: Affine, threads: u16) -> Rendered {
    render_with(path, fill, stroke, xf, threads, 1.0, Some(BLUE))
}

fn render_with(path: PathData, fill: bool, stroke: Stroke, xf: Affine, threads: u16, opacity: f32, background: Option<[u8; 4]>) -> Rendered {
    let mut d = Document::new(&NewDocument { width: 96.0, height: 96.0, facing_pages: false, ..Default::default() });
    d.swatches.push(Swatch::color("Red", Color::rgb8(255, 0, 0)));
    let id = ItemId(d.alloc());
    let mut item = Item::new(id, d.default_layer(), Shape::Rectangle, path);
    item.fill = if fill { Fill::swatch(designcraft_color::swatch::PAPER) } else { Fill::default() };
    item.stroke = stroke;
    item.xf = xf;
    item.opacity = opacity;
    d.insert_item(SpreadRef::Doc(0), item, None).unwrap();
    let mut renderer = Renderer::new();
    renderer.threads = threads;
    renderer.render(
        &d,
        &Cache::new(),
        &[Placed { spread: SpreadRef::Doc(0), xf: Affine::IDENTITY }],
        96,
        96,
        Affine::IDENTITY,
        &RenderOptions { paper: false, background, ..Default::default() },
    )
}

fn stroke(align: StrokeAlign) -> Stroke {
    Stroke { swatch: "Red".into(), weight: 6.0, align, ..Default::default() }
}

fn rectangle() -> PathData {
    shapes::rectangle(Rect::new(24.0, 24.0, 72.0, 72.0))
}

#[test]
fn outside_stroke_keeps_white_center_and_does_not_cover_fill() {
    let img = render(rectangle(), true, stroke(StrokeAlign::Outside), Affine::IDENTITY, 0);
    assert_eq!(img.pixel(48, 48), WHITE, "an outside stroke must not paint the center");
    assert_eq!(img.pixel(26, 48), WHITE, "the inside half of the doubled stroke is excluded");
    assert_eq!(img.pixel(20, 48), RED, "the full stroke weight is outside the path");
    assert_eq!(img.pixel(16, 48), BLUE, "the outside stroke remains bounded");
}

#[test]
fn outside_stroke_preserves_backdrop_without_a_fill() {
    let img = render(rectangle(), false, stroke(StrokeAlign::Outside), Affine::IDENTITY, 0);
    assert_eq!(img.pixel(48, 48), BLUE);
    assert_eq!(img.pixel(26, 48), BLUE, "removing the inner stroke must not erase the backdrop");
    assert_eq!(img.pixel(20, 48), RED);
}

#[test]
fn outside_stroke_preserves_transparency() {
    let img = render_with(rectangle(), false, stroke(StrokeAlign::Outside), Affine::IDENTITY, 0, 1.0, None);
    assert_eq!(img.pixel(48, 48), [0; 4]);
    assert_eq!(img.pixel(26, 48), [0; 4]);
    assert_eq!(img.pixel(20, 48), RED);
    assert_eq!(img.pixel(16, 48), [0; 4]);
}

#[test]
fn outside_stroke_applies_item_opacity_once() {
    let img = render_with(rectangle(), true, stroke(StrokeAlign::Outside), Affine::IDENTITY, 0, 0.5, None);
    for (x, expected) in [(48, [255, 255, 255]), (26, [255, 255, 255]), (20, [255, 0, 0])] {
        let pixel = img.pixel(x, 48);
        assert_eq!(&pixel[..3], &expected, "colour at {x}");
        assert!(pixel[3].abs_diff(128) <= 1, "opacity at {x}: {pixel:?}");
    }
    assert_eq!(img.pixel(16, 48), [0; 4]);
}

#[test]
fn outside_stroke_follows_compound_holes_and_nonzero_winding() {
    // Opposite winding creates a hole. Its boundary strokes toward the hole.
    let mut path = rectangle();
    let mut hole = shapes::rectangle(Rect::new(36.0, 36.0, 60.0, 60.0));
    for subpath in &mut hole.subpaths {
        subpath.reverse();
    }
    path.subpaths.extend(hole.subpaths);
    let img = render(path, true, stroke(StrokeAlign::Outside), Affine::IDENTITY, 0);
    assert_eq!(img.pixel(30, 48), WHITE, "stroke must stay out of the filled area");
    assert_eq!(img.pixel(38, 48), RED, "the hole is outside the compound shape");
    assert_eq!(img.pixel(48, 48), BLUE, "the hole center remains unpainted");
    assert_eq!(img.pixel(20, 48), RED);

    // Same-winding nested paths do not make a hole under the renderer's nonzero fill rule.
    let mut path = rectangle();
    path.subpaths.extend(shapes::rectangle(Rect::new(36.0, 36.0, 60.0, 60.0)).subpaths);
    let img = render(path, true, stroke(StrokeAlign::Outside), Affine::IDENTITY, 0);
    assert_eq!(img.pixel(38, 48), WHITE, "nested filled contours must not acquire interior ink");
    assert_eq!(img.pixel(48, 48), WHITE);
    assert_eq!(img.pixel(20, 48), RED);
}

#[test]
fn stroke_alignment_preserves_widths_and_open_path_behavior() {
    for (align, exterior, interior) in [(StrokeAlign::Outside, RED, WHITE), (StrokeAlign::Inside, BLUE, RED), (StrokeAlign::Center, BLUE, WHITE)] {
        let img = render(rectangle(), true, stroke(align), Affine::IDENTITY, 0);
        assert_eq!(img.pixel(20, 48), exterior, "{align:?} exterior");
        assert_eq!(img.pixel(28, 48), interior, "{align:?} interior");
        assert_eq!(img.pixel(48, 48), WHITE);
    }
    let line = shapes::line(Point::new(24.0, 48.0), Point::new(72.0, 48.0));
    let centered = render(line.clone(), false, stroke(StrokeAlign::Center), Affine::IDENTITY, 0);
    for align in [StrokeAlign::Inside, StrokeAlign::Outside] {
        let img = render(line.clone(), false, stroke(align), Affine::IDENTITY, 0);
        assert_eq!(img.pixels, centered.pixels, "open paths always use centered strokes");
    }
}

#[test]
fn outside_stroke_retains_joins_dashes_and_transforms() {
    for join in [Join::Miter, Join::Round, Join::Bevel] {
        let mut st = stroke(StrokeAlign::Outside);
        st.join = join;
        let img = render(rectangle(), true, st, Affine::IDENTITY, 0);
        assert_eq!(img.pixel(48, 48), WHITE);
        assert_eq!(img.pixel(20, 48), RED);
        assert_eq!(img.pixel(18, 18), if join == Join::Miter { RED } else { BLUE }, "{join:?} corner");
    }
    let mut dashed = stroke(StrokeAlign::Outside);
    dashed.kind = StrokeType::Dashed { pattern: vec![12.0, 12.0] };
    let img = render(rectangle(), true, dashed, Affine::IDENTITY, 0);
    assert_eq!(img.pixel(30, 20), RED, "dash stays outside");
    assert_eq!(img.pixel(42, 20), BLUE, "dash gap stays empty");
    assert_eq!(img.pixel(30, 26), WHITE, "dash stays out of the fill");
    assert_eq!(img.pixel(48, 48), WHITE);

    let xf = Affine::translate((48.0, 48.0)) * Affine::rotate(std::f64::consts::FRAC_PI_4) * Affine::translate((-48.0, -48.0));
    let img = render(rectangle(), true, stroke(StrokeAlign::Outside), xf, 0);
    assert_eq!(img.pixel(48, 48), WHITE);
    assert_eq!(img.pixel(28, 28), RED);
    assert_eq!(img.pixel(34, 34), WHITE);
    assert_eq!(img.pixel(22, 22), BLUE);
}

#[test]
fn outside_stroke_is_identical_with_multiple_render_threads() {
    let single = render(rectangle(), true, stroke(StrokeAlign::Outside), Affine::IDENTITY, 0);
    let multi = render(rectangle(), true, stroke(StrokeAlign::Outside), Affine::IDENTITY, 2);
    assert_eq!(single.pixels, multi.pixels);
}
