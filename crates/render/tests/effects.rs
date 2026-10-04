//! Golden images for the soft object effects (drop shadow, outer glow, inner shadow, feather),
//! plus property checks and a multithreaded == single-threaded check.
//!
//! Goldens live in `tests/golden/*.png` and are compared with a tolerance (antialiasing and blur
//! rounding may differ by a few levels between SIMD levels). Regenerate them after an intended
//! change with `DESIGNCRAFT_BLESS=1 cargo test -p designcraft-render --test effects`.

use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Document, Fill, Item, ItemId, ParaFormat, Shape, SpreadRef};
use designcraft_geom::{Affine, Rect, Vec2, shapes};
use designcraft_render::{Placed, RenderOptions, Rendered, Renderer};

const SIZE: u32 = 160;

fn doc() -> Document {
    Document::new(&NewDocument { width: 160.0, height: 160.0, facing_pages: false, ..Default::default() })
}

fn rect_item(d: &mut Document, r: Rect, swatch: &str) -> ItemId {
    let id = ItemId(d.alloc());
    let lid = d.default_layer();
    let mut it = Item::new(id, lid, Shape::Rectangle, shapes::rectangle(r));
    it.fill = Fill::swatch(swatch);
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    id
}

fn render_with(d: &Document, threads: u16) -> Rendered {
    let mut r = Renderer::new();
    r.threads = threads;
    let opts = RenderOptions { background: Some([255, 255, 255, 255]), ..Default::default() };
    r.render(d, &Cache::new(), &[Placed { spread: SpreadRef::Doc(0), xf: Affine::translate(Vec2::ZERO) }], SIZE, SIZE, Affine::IDENTITY, &opts)
}

fn render(d: &Document) -> Rendered {
    render_with(d, 0)
}

fn lum(p: [u8; 4]) -> u32 {
    p[0] as u32 + p[1] as u32 + p[2] as u32
}

const WHITE: u32 = 765;

/// Tolerant comparison with `tests/golden/{name}.png` (written when missing or when blessing).
fn golden(name: &str, img: &Rendered) {
    let path = format!("{}/tests/golden/{name}.png", env!("CARGO_MANIFEST_DIR"));
    let bless = std::env::var_os("DESIGNCRAFT_BLESS").is_some();
    let Ok(bytes) = std::fs::read(&path).map_err(|_| ()).and_then(|b| if bless { Err(()) } else { Ok(b) }) else {
        std::fs::create_dir_all(format!("{}/tests/golden", env!("CARGO_MANIFEST_DIR"))).unwrap();
        std::fs::write(&path, img.to_png()).unwrap();
        eprintln!("wrote golden {path}");
        return;
    };
    let want = image::load_from_memory(&bytes).unwrap().to_rgba8();
    assert_eq!((want.width(), want.height()), (img.width, img.height), "{name}: size");
    let got = img.to_straight();
    let (mut bad, mut sum, mut worst) = (0usize, 0u64, 0u8);
    for (a, b) in got.as_chunks::<4>().0.iter().zip(want.as_raw().as_chunks::<4>().0) {
        let d = a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap();
        sum += d as u64;
        worst = worst.max(d);
        if d > 8 {
            bad += 1;
        }
    }
    let n = (img.width * img.height) as usize;
    let mean = sum as f64 / n as f64;
    if bad * 200 > n || mean > 1.0 {
        let out = std::env::temp_dir().join(format!("designcraft-golden-{name}.png"));
        let _ = std::fs::write(&out, img.to_png());
        panic!("{name}: {bad} pixels differ by > 8 (worst {worst}, mean {mean:.2}); got {}", out.display());
    }
}

fn shadow_doc(spread: f64) -> Document {
    let mut d = doc();
    let id = rect_item(&mut d, Rect::new(30.0, 30.0, 100.0, 100.0), "C=100 M=0 Y=0 K=0");
    let ds = &mut d.item_mut(id).unwrap().effects.drop_shadow;
    ds.on = true;
    ds.distance = 12.0;
    ds.angle = 135.0;
    ds.size = 10.0;
    ds.spread = spread;
    ds.opacity = 0.8;
    d
}

#[test]
fn drop_shadow_is_soft_offset_and_under_the_object() {
    let img = render(&shadow_doc(0.0));
    golden("drop-shadow", &img);
    // The object is unchanged on top.
    let c = img.pixel(60, 60);
    assert!(c[0] < 60 && c[2] > 200, "object on top: {c:?}");
    // Below-right is shadowed, above-left is not.
    assert!(lum(img.pixel(104, 70)) < 500, "shadow below-right: {:?}", img.pixel(104, 70));
    assert_eq!(lum(img.pixel(22, 22)), WHITE);
    // Soft: the shadow fades out gradually past its hard edge (100 + 8.5 offset).
    let row: Vec<u32> = (104..130).map(|x| lum(img.pixel(x, 80))).collect();
    let mid = row.iter().filter(|&&v| v > 60 && v < 700).count();
    assert!(mid >= 6, "gradual falloff: {row:?}");
    assert!(row.windows(2).all(|w| w[1] + 2 >= w[0]), "monotonic: {row:?}");
}

#[test]
fn drop_shadow_spread_hardens_and_grows() {
    let soft = render(&shadow_doc(0.0));
    let hard = render(&shadow_doc(100.0));
    golden("drop-shadow-spread", &hard);
    // Full spread: no blur, the silhouette grows by `size` → solid well past the offset edge.
    let x = 100 + 8 + 6;
    assert!(lum(hard.pixel(x, 80)) < lum(soft.pixel(x, 80)), "spread grows the shadow");
    assert_eq!(lum(hard.pixel(126, 80)), WHITE, "hard edge");
}

#[test]
fn drop_shadow_of_text_follows_the_glyphs() {
    let mut d = doc();
    let lid = d.default_layer();
    let (fid, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(10.0, 10.0, 150.0, 150.0), lid, "Shadow", ParaFormat::default()).unwrap();
    d.story_mut(sid).unwrap().format_chars(0..6, |f| f.over.size = Some(40.0));
    let ds = &mut d.item_mut(fid).unwrap().effects.drop_shadow;
    ds.on = true;
    ds.distance = 4.0;
    ds.size = 3.0;
    let img = render(&d);
    golden("drop-shadow-text", &img);
    // No frame-shaped shadow: the bottom-right of the (unfilled) frame is clear.
    assert_eq!(lum(img.pixel(140, 140)), WHITE);
}

#[test]
fn outer_glow_surrounds_the_object() {
    let mut d = doc();
    let id = rect_item(&mut d, Rect::new(50.0, 50.0, 110.0, 110.0), "[Black]");
    let g = &mut d.item_mut(id).unwrap().effects.outer_glow;
    g.on = true;
    g.size = 12.0;
    g.color = "C=0 M=100 Y=0 K=0".into();
    g.opacity = 1.0;
    let img = render(&d);
    golden("outer-glow", &img);
    for (x, y) in [(46, 80), (114, 80), (80, 46), (80, 114)] {
        let p = img.pixel(x, y);
        assert!(p[1] < 200 && p[0] > p[1] + 40, "magenta glow at {x},{y}: {p:?}");
    }
    assert_eq!(lum(img.pixel(10, 10)), WHITE);
}

#[test]
fn inner_shadow_darkens_inside_the_lit_edge() {
    let mut d = doc();
    let id = rect_item(&mut d, Rect::new(30.0, 30.0, 130.0, 130.0), "C=0 M=0 Y=100 K=0");
    let s = &mut d.item_mut(id).unwrap().effects.inner_shadow;
    s.on = true;
    s.distance = 8.0;
    s.size = 8.0;
    s.opacity = 1.0;
    let img = render(&d);
    golden("inner-shadow", &img);
    // Light from the top-left: the shadow sits inside the top-left edges.
    assert!(lum(img.pixel(33, 80)) + 100 < lum(img.pixel(126, 80)), "{:?} vs {:?}", img.pixel(33, 80), img.pixel(126, 80));
    assert!(lum(img.pixel(80, 33)) + 100 < lum(img.pixel(80, 126)));
    // Nothing outside the shape.
    assert_eq!(lum(img.pixel(26, 80)), WHITE);
}

#[test]
fn feather_fades_the_edge_inward() {
    let mut d = doc();
    let id = rect_item(&mut d, Rect::new(30.0, 30.0, 130.0, 130.0), "[Black]");
    d.item_mut(id).unwrap().effects.feather = 16.0;
    let img = render(&d);
    golden("feather", &img);
    d.item_mut(id).unwrap().effects.feather = 0.0;
    let solid = lum(render(&d).pixel(80, 80));
    let edge = lum(img.pixel(30, 80));
    let mid = lum(img.pixel(38, 80));
    let inside = lum(img.pixel(80, 80));
    assert!(edge > 600, "nearly transparent at the path: {edge}");
    assert!(mid > solid + 60 && mid < 650, "half way: {mid}");
    assert!(inside <= solid + 6, "solid inside: {inside} vs {solid}");
    assert_eq!(lum(img.pixel(27, 80)), WHITE, "nothing outside");
}

#[test]
fn shadow_of_an_object_outside_the_view_still_reaches_it() {
    let mut d = doc();
    let id = rect_item(&mut d, Rect::new(-60.0, 40.0, -2.0, 120.0), "[Paper]");
    let ds = &mut d.item_mut(id).unwrap().effects.drop_shadow;
    ds.on = true;
    ds.angle = 180.0;
    ds.distance = 20.0;
    ds.size = 2.0;
    ds.opacity = 1.0;
    let img = render(&d);
    assert!(lum(img.pixel(8, 80)) < 200, "{:?}", img.pixel(8, 80));
}

/// Multithreaded contexts draw effects offscreen; the result must match single-threaded.
#[test]
fn multithreaded_effects_match_single_threaded() {
    let mut docs = vec![shadow_doc(0.0), shadow_doc(40.0)];
    let mut d = doc();
    rect_item(&mut d, Rect::new(0.0, 0.0, 160.0, 160.0), "C=0 M=30 Y=60 K=0");
    let id = rect_item(&mut d, Rect::new(30.0, 30.0, 130.0, 130.0), "C=100 M=0 Y=0 K=0");
    {
        let it = d.item_mut(id).unwrap();
        it.effects.feather = 10.0;
        it.effects.inner_shadow.on = true;
        it.effects.outer_glow.on = true;
        it.effects.drop_shadow.on = true;
        it.opacity = 0.8;
    }
    docs.push(d);
    for (i, d) in docs.iter().enumerate() {
        let a = render_with(d, 0);
        let b = render_with(d, 3);
        let worst = a.pixels.iter().zip(&b.pixels).map(|(x, y)| x.abs_diff(*y)).max().unwrap();
        assert!(worst <= 4, "case {i}: max channel difference {worst}");
    }
}
