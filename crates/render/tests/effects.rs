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
    ds.angle = 135.0;
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
    s.angle = 135.0;
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

/// Points 70..90 of the page at 8 px a point (400% on a 2x screen), where a wide blur is drawn
/// offscreen at a lower resolution.
fn render_zoomed(d: &Document) -> Rendered {
    let mut r = Renderer::new();
    let opts = RenderOptions { background: Some([255, 255, 255, 255]), ..Default::default() };
    let view = Affine::scale(8.0) * Affine::translate(Vec2::new(-70.0, -70.0));
    r.render(d, &Cache::new(), &[Placed { spread: SpreadRef::Doc(0), xf: Affine::translate(Vec2::ZERO) }], SIZE, SIZE, view, &opts)
}

#[test]
fn a_wide_feather_keeps_the_object_sharp_at_high_zoom() {
    let mut d = doc();
    // A huge yellow frame holding a small black square (pt 78..82, px 64..96 when zoomed).
    let id = rect_item(&mut d, Rect::new(-5000.0, -5000.0, 5160.0, 5160.0), "C=0 M=0 Y=100 K=0");
    let kid = Item {
        fill: Fill::swatch("[Black]"),
        ..Item::new(ItemId(d.alloc()), d.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(78.0, 78.0, 82.0, 82.0)))
    };
    d.item_mut(id).unwrap().content = designcraft_doc::Content::Group { items: vec![std::sync::Arc::new(kid)] };
    let plain = render_zoomed(&d);
    // 3σ of a 1,000 pt feather is 6,000 px here, so its mask is drawn at about 1/6 resolution.
    d.item_mut(id).unwrap().effects.feather = 1000.0;
    let img = render_zoomed(&d);
    let worst = plain.pixels.iter().zip(&img.pixels).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
    assert!(worst <= 4, "the inside of a feathered object matches the object: max channel difference {worst}");
}

#[test]
fn a_wide_inner_effect_stays_inside_the_shape_at_high_zoom() {
    let mut d = doc();
    // A black backdrop shows a screened glow; white paper shows a shadow.
    let backdrop = rect_item(&mut d, Rect::new(0.0, 0.0, 160.0, 160.0), "[Black]");
    // The shape's right edge is at pt 80, px 80 when zoomed.
    let id = rect_item(&mut d, Rect::new(-2000.0, -2000.0, 80.0, 2160.0), "C=100 M=0 Y=0 K=0");
    for glow in [false, true] {
        d.item_mut(backdrop).unwrap().fill = Fill::swatch(if glow { "[Black]" } else { "[None]" });
        let plain = render_zoomed(&d);
        let e = &mut d.item_mut(id).unwrap().effects;
        // 3σ is 24,000 px, so the effect is drawn at about 1/23 resolution.
        if glow {
            e.inner_glow.on = true;
            e.inner_glow.size = 2000.0;
            e.inner_glow.opacity = 1.0;
        } else {
            e.inner_shadow.on = true;
            e.inner_shadow.color = "[Black]".into();
            e.inner_shadow.distance = 0.0;
            e.inner_shadow.size = 2000.0;
            e.inner_shadow.opacity = 1.0;
        }
        let img = render_zoomed(&d);
        assert_ne!(lum(img.pixel(78, 80)), lum(plain.pixel(78, 80)), "glow {glow}: the effect shows inside the edge");
        for x in 81..110 {
            assert_eq!(lum(img.pixel(x, 80)), lum(plain.pixel(x, 80)), "glow {glow}: nothing outside the edge at x {x}");
        }
        let e = &mut d.item_mut(id).unwrap().effects;
        e.inner_glow.on = false;
        e.inner_shadow.on = false;
    }
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

/// A frame (30..130) holding a pasted-in black square that overhangs it (Edit › Paste Into).
fn pasted_into(d: &mut Document) -> ItemId {
    let id = rect_item(d, Rect::new(30.0, 30.0, 130.0, 130.0), "[None]");
    let kid = Item {
        fill: Fill::swatch("[Black]"),
        ..Item::new(ItemId(d.alloc()), d.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 160.0, 160.0)))
    };
    d.item_mut(id).unwrap().content = designcraft_doc::Content::Group { items: vec![std::sync::Arc::new(kid)] };
    assert!(d.item(id).unwrap().has_nested_items());
    id
}

#[test]
fn effects_apply_to_a_frame_with_pasted_in_items() {
    let mut d = doc();
    let id = pasted_into(&mut d);
    let plain = render(&d);
    let black = lum(plain.pixel(80, 80));
    assert!(black < 200 && lum(plain.pixel(31, 80)) <= black + 6, "the content fills the frame: {:?} {:?}", plain.pixel(31, 80), plain.pixel(80, 80));
    assert_eq!(lum(plain.pixel(27, 80)), WHITE, "clipped by the frame");
    // Basic Feather fades the frame's edge, content and all.
    d.item_mut(id).unwrap().effects.feather = 16.0;
    let img = render(&d);
    assert!(lum(img.pixel(30, 80)) > 600, "nearly transparent at the path: {:?}", img.pixel(30, 80));
    assert!(lum(img.pixel(80, 80)) <= black + 6, "solid inside");
    assert_eq!(lum(img.pixel(27, 80)), WHITE, "still clipped");
    // Inner Glow: white screened in from the edges.
    d.item_mut(id).unwrap().effects.feather = 0.0;
    let g = &mut d.item_mut(id).unwrap().effects.inner_glow;
    g.on = true;
    g.size = 12.0;
    g.opacity = 1.0;
    let img = render(&d);
    assert!(lum(img.pixel(32, 80)) > black + 150, "lit at the edge: {:?}", img.pixel(32, 80));
    assert!(lum(img.pixel(80, 80)) <= black + 6, "dark in the middle");
    // Drop shadow: cast by the content's silhouette, outside the frame.
    d.item_mut(id).unwrap().effects.inner_glow.on = false;
    let ds = &mut d.item_mut(id).unwrap().effects.drop_shadow;
    ds.on = true;
    ds.angle = 180.0;
    ds.global_light = false;
    ds.distance = 12.0;
    ds.size = 0.0;
    ds.opacity = 1.0;
    let img = render(&d);
    assert!(lum(img.pixel(136, 80)) < 300, "the shadow to the right: {:?}", img.pixel(136, 80));
}

#[test]
fn inner_glow_mode_and_technique() {
    let mut d = doc();
    let id = rect_item(&mut d, Rect::new(30.0, 30.0, 130.0, 130.0), "[Paper]");
    let g = &mut d.item_mut(id).unwrap().effects.inner_glow;
    g.on = true;
    g.color = "[Black]".into();
    g.opacity = 1.0;
    g.size = 20.0;
    // Screen (the default) can't darken white paper; Multiply can.
    let screen = render(&d);
    assert!(lum(screen.pixel(32, 80)) > 740, "{:?}", screen.pixel(32, 80));
    d.item_mut(id).unwrap().effects.inner_glow.mode = designcraft_color::BlendMode::Multiply;
    let soft = render(&d);
    assert!(lum(soft.pixel(32, 80)) + 150 < lum(screen.pixel(32, 80)), "a dark edge: {:?}", soft.pixel(32, 80));
    // Precise keeps the glow nearer the edge than Softer.
    d.item_mut(id).unwrap().effects.inner_glow.technique = designcraft_doc::GlowTechnique::Precise;
    let precise = render(&d);
    assert!(lum(precise.pixel(42, 80)) > lum(soft.pixel(42, 80)) + 30, "{:?} vs {:?}", precise.pixel(42, 80), soft.pixel(42, 80));
}

#[test]
fn feather_choke_and_corners() {
    let mut d = doc();
    let id = rect_item(&mut d, Rect::new(30.0, 30.0, 130.0, 130.0), "[Black]");
    d.item_mut(id).unwrap().effects.feather = 16.0;
    let diffused = render(&d);
    golden("feather", &diffused);
    // Choke: half the width stays opaque, so the fade is shorter.
    d.item_mut(id).unwrap().effects.feather_options.choke = 50.0;
    let choked = render(&d);
    assert!(lum(choked.pixel(38, 80)) + 60 < lum(diffused.pixel(38, 80)), "{:?} vs {:?}", choked.pixel(38, 80), diffused.pixel(38, 80));
    assert!(lum(choked.pixel(30, 80)) > 500, "still transparent at the path");
    // Sharp corners keep more of the corner than Rounded ones (past the fade, near the corner).
    d.item_mut(id).unwrap().effects.feather_options = designcraft_doc::FeatherOptions { choke: 0.0, corners: designcraft_doc::FeatherCorners::Sharp };
    let sharp = render(&d);
    d.item_mut(id).unwrap().effects.feather_options.corners = designcraft_doc::FeatherCorners::Rounded;
    let rounded = render(&d);
    assert!(lum(sharp.pixel(41, 41)) + 20 < lum(rounded.pixel(41, 41)), "{:?} vs {:?}", sharp.pixel(41, 41), rounded.pixel(41, 41));
}

/// A shadowed square and a shadowed text frame, for the kept-shadow tests.
fn shadowed_text_doc() -> (Document, ItemId, designcraft_doc::StoryId) {
    let mut d = shadow_doc(20.0);
    let lid = d.default_layer();
    let (tid, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(20.0, 105.0, 150.0, 150.0), lid, "Shadow", ParaFormat::default()).unwrap();
    let ds = &mut d.item_mut(tid).unwrap().effects.drop_shadow;
    ds.on = true;
    ds.distance = 3.0;
    ds.size = 2.0;
    ds.opacity = 0.9;
    (d, tid, sid)
}

/// One render of `d` through `view` with the composition `cache`, which an app keeps between
/// renders as it keeps its renderer.
fn render_view(r: &mut Renderer, cache: &Cache, d: &Document, view: Affine) -> Rendered {
    let opts = RenderOptions { background: Some([255, 255, 255, 255]), ..Default::default() };
    r.render(d, cache, &[Placed { spread: SpreadRef::Doc(0), xf: Affine::IDENTITY }], SIZE, SIZE, view, &opts)
}

fn fresh(d: &Document, view: Affine) -> Rendered {
    let mut r = Renderer::new();
    r.threads = 3;
    render_view(&mut r, &Cache::new(), d, view)
}

fn worst(a: &Rendered, b: &Rendered) -> u8 {
    a.pixels.iter().zip(&b.pixels).map(|(x, y)| x.abs_diff(*y)).max().unwrap()
}

fn mean(a: &Rendered, b: &Rendered) -> f64 {
    a.pixels.iter().zip(&b.pixels).map(|(x, y)| f64::from(x.abs_diff(*y))).sum::<f64>() / a.pixels.len() as f64
}

/// Multithreaded renders keep drop shadows and glows between frames: placed again, they look as
/// drawn afresh, also after panning, and an edit draws them anew.
#[test]
fn kept_shadows_look_as_drawn_afresh() {
    let (mut d, tid, sid) = shadowed_text_doc();
    let (mut r, cache) = (Renderer::new(), Cache::new());
    r.threads = 3;
    let first = render_view(&mut r, &cache, &d, Affine::IDENTITY);
    assert_eq!(render_view(&mut r, &cache, &d, Affine::IDENTITY).pixels, first.pixels, "placed again unchanged");
    // Panned by whole pixels: as drawn afresh.
    let view = Affine::translate((13.0, -7.0));
    let w = worst(&render_view(&mut r, &cache, &d, view), &fresh(&d, view));
    assert!(w <= 4, "panned by whole pixels: max channel difference {w}");
    // By fractions of a pixel the kept image is resampled: a few levels at shadow edges.
    let view = Affine::translate((-4.5, 6.25));
    let kept = render_view(&mut r, &cache, &d, view);
    let (w, m) = (worst(&kept, &fresh(&d, view)), mean(&kept, &fresh(&d, view)));
    assert!(w <= 24 && m < 1.0, "panned by fractions: max channel difference {w}, mean {m:.3}");
    // Zoomed: drawn anew.
    let zoom = Affine::scale(1.5);
    assert!(worst(&render_view(&mut r, &cache, &d, zoom), &fresh(&d, zoom)) <= 4);
    // Zoomed on (kept without a margin while zooming), then panned and zoomed back.
    for view in [Affine::scale(1.6), Affine::translate((13.0, -7.0)) * Affine::scale(1.6), Affine::IDENTITY] {
        assert!(worst(&render_view(&mut r, &cache, &d, view), &fresh(&d, view)) <= 4, "{view:?}");
    }
    // Edits: an object without text (only the object itself tells), the shadow, the text and the
    // frame, each after a render that kept what it drew.
    render_view(&mut r, &cache, &d, Affine::IDENTITY);
    let square = d.spreads[0].items.iter().find(|i| i.id != tid && i.effects.drop_shadow.on).unwrap().id;
    // (Its opacity: the same extent, so nothing but the object itself has changed.)
    d.item_mut(square).unwrap().effects.drop_shadow.opacity = 0.3;
    assert!(worst(&render_view(&mut r, &cache, &d, Affine::IDENTITY), &fresh(&d, Affine::IDENTITY)) <= 4, "square's shadow edited");
    render_view(&mut r, &cache, &d, Affine::IDENTITY);
    d.item_mut(tid).unwrap().effects.drop_shadow.opacity = 0.3;
    assert!(worst(&render_view(&mut r, &cache, &d, Affine::IDENTITY), &fresh(&d, Affine::IDENTITY)) <= 4, "shadow edited");
    render_view(&mut r, &cache, &d, Affine::IDENTITY);
    let story = d.story_mut(sid).unwrap();
    let fmt = story.char_format_at(0).clone();
    story.insert_with(0, "Long ", fmt);
    assert!(worst(&render_view(&mut r, &cache, &d, Affine::IDENTITY), &fresh(&d, Affine::IDENTITY)) <= 4, "text edited");
    render_view(&mut r, &cache, &d, Affine::IDENTITY);
    d.item_mut(tid).unwrap().xf = Affine::translate((0.0, -40.0));
    assert!(worst(&render_view(&mut r, &cache, &d, Affine::IDENTITY), &fresh(&d, Affine::IDENTITY)) <= 4, "frame moved");
    // A swatch the shadows use (their colour, [Black]), changed in the document.
    render_view(&mut r, &cache, &d, Affine::IDENTITY);
    let black = d.swatches.iter_mut().find(|s| s.name == "[Black]").unwrap();
    if let designcraft_color::SwatchValue::Color { color, .. } = &mut black.value {
        *color = designcraft_color::Color::rgb(1.0, 0.0, 0.0);
    }
    assert!(worst(&render_view(&mut r, &cache, &d, Affine::IDENTITY), &fresh(&d, Affine::IDENTITY)) <= 4, "swatch edited");
}

/// A kept shadow of a stroke in a stroke style of the document is drawn anew when the style changes.
#[test]
fn kept_shadows_follow_an_edited_stroke_style() {
    use designcraft_doc::{Stroke, StrokeStyleDef, StrokeType};
    let mut d = doc();
    let id = rect_item(&mut d, Rect::new(30.0, 30.0, 110.0, 110.0), "C=100 M=0 Y=0 K=0");
    let style = |pattern: Vec<f64>| StrokeStyleDef { name: "Dashes".into(), kind: StrokeType::Dashed { pattern } };
    d.stroke_styles.push(style(vec![12.0, 12.0]));
    let it = d.item_mut(id).unwrap();
    it.fill = Fill::none();
    it.stroke = Stroke { weight: 10.0, kind: StrokeType::Style { name: "Dashes".into() }, ..Default::default() };
    let ds = &mut it.effects.drop_shadow;
    ds.on = true;
    ds.distance = 8.0;
    ds.size = 2.0;
    ds.opacity = 1.0;
    let (mut r, cache) = (Renderer::new(), Cache::new());
    r.threads = 3;
    render_view(&mut r, &cache, &d, Affine::IDENTITY);
    render_view(&mut r, &cache, &d, Affine::IDENTITY);
    d.stroke_styles[0] = style(vec![3.0, 20.0]);
    let w = worst(&render_view(&mut r, &cache, &d, Affine::IDENTITY), &fresh(&d, Affine::IDENTITY));
    assert!(w <= 4, "style edited: max channel difference {w}");
}

/// A shadow too large to keep whole at a high zoom keeps what is on screen and a margin; panning
/// past that draws it anew.
#[test]
fn large_kept_shadows_are_redrawn_where_panning_leaves_them() {
    let d = shadow_doc(0.0);
    let (mut r, cache) = (Renderer::new(), Cache::new());
    r.threads = 3;
    // 40×: the square's shadow is thousands of pixels wide. The view starts on the square's
    // right edge (x 100 pt at 80 px) and pans across the shadow's soft edge, by less and by more
    // than what is kept around the screen.
    for x in [0.0, -100.0, -300.0, -600.0, -150.0, 200.0] {
        let view = Affine::translate((80.0 - 4000.0 + x, -2600.0)) * Affine::scale(40.0);
        let w = worst(&render_view(&mut r, &cache, &d, view), &fresh(&d, view));
        assert!(w <= 12, "at {x}: max channel difference {w}");
    }
}

/// Every kind of soft effect, on several objects, drawn on a multithreaded context: the effect
/// images are drawn together on the cores and kept. Drawn so, placed again, panned and after an
/// edit, they look as drawn on a single thread.
#[test]
fn all_effects_drawn_together_and_kept_look_as_drawn_one_by_one() {
    let mut d = doc();
    let mut ids = vec![];
    for (k, r) in [
        Rect::new(20.0, 20.0, 70.0, 70.0),
        Rect::new(90.0, 20.0, 140.0, 70.0),
        Rect::new(20.0, 90.0, 70.0, 140.0),
        Rect::new(90.0, 90.0, 140.0, 140.0),
    ]
    .into_iter()
    .enumerate()
    {
        let id = rect_item(&mut d, r, if k % 2 == 0 { "C=100 M=0 Y=0 K=0" } else { "C=0 M=100 Y=0 K=0" });
        let e = &mut d.item_mut(id).unwrap().effects;
        match k {
            0 => {
                e.drop_shadow.on = true;
                e.inner_shadow.on = true;
            }
            1 => {
                e.bevel.on = true;
                e.bevel.size = 6.0;
                e.outer_glow.on = true;
            }
            2 => e.feather = 8.0,
            _ => {
                e.inner_glow.on = true;
                e.satin.on = true;
            }
        }
        ids.push(id);
    }
    let one_by_one = |d: &Document, view: Affine| {
        let mut r = Renderer::new();
        r.threads = 0;
        render_view(&mut r, &Cache::new(), d, view)
    };
    let (mut r, cache) = (Renderer::new(), Cache::new());
    r.threads = 3;
    let first = render_view(&mut r, &cache, &d, Affine::IDENTITY);
    let w = worst(&first, &one_by_one(&d, Affine::IDENTITY));
    assert!(w <= 4, "drawn together: max channel difference {w}");
    assert_eq!(render_view(&mut r, &cache, &d, Affine::IDENTITY).pixels, first.pixels, "placed again unchanged");
    let view = Affine::translate((11.0, -6.0));
    let w = worst(&render_view(&mut r, &cache, &d, view), &one_by_one(&d, view));
    assert!(w <= 4, "panned: max channel difference {w}");
    let zoom = Affine::scale(1.5);
    let w = worst(&render_view(&mut r, &cache, &d, zoom), &one_by_one(&d, zoom));
    assert!(w <= 4, "zoomed: max channel difference {w}");
    // Edited, each effect as far as before (only the objects themselves tell): the bevel's
    // highlight and the inner glow fainter.
    render_view(&mut r, &cache, &d, Affine::IDENTITY);
    d.item_mut(ids[1]).unwrap().effects.bevel.highlight_opacity = 0.2;
    d.item_mut(ids[3]).unwrap().effects.inner_glow.opacity = 0.2;
    let w = worst(&render_view(&mut r, &cache, &d, Affine::IDENTITY), &one_by_one(&d, Affine::IDENTITY));
    assert!(w <= 4, "edited: max channel difference {w}");
}

/// Zoomed in, a wide blur is drawn at a lower resolution and scaled up: it looks as drawn at full
/// resolution on a single thread, within a few levels.
#[test]
fn wide_blurs_drawn_coarser_look_as_drawn_at_full_resolution() {
    let mut d = shadow_doc(0.0);
    let id = rect_item(&mut d, Rect::new(110.0, 30.0, 150.0, 70.0), "C=0 M=100 Y=0 K=0");
    {
        let e = &mut d.item_mut(id).unwrap().effects;
        e.bevel.on = true;
        e.bevel.size = 6.0;
        e.inner_shadow.on = true;
        e.outer_glow.on = true;
    }
    // 6×: the shadow's σ is 30 pixels, drawn at a tenth of the resolution.
    let view = Affine::translate((-560.0, -100.0)) * Affine::scale(6.0);
    let mut r = Renderer::new();
    r.threads = 3;
    let coarse = render_view(&mut r, &Cache::new(), &d, view);
    let mut one = Renderer::new();
    one.threads = 0;
    let full = render_view(&mut one, &Cache::new(), &d, view);
    let (w, m) = (worst(&coarse, &full), mean(&coarse, &full));
    // (Shadows are coloured as they are placed, rounded a level apart from vello's filter.)
    assert!(w <= 10 && m < 1.0, "max channel difference {w}, mean {m:.3}");
}

/// A large effect image is drawn in bands of rows on several cores, each reaching past its rows
/// by the blur's spread: no seams, as drawn whole on a single thread; placed again from what was
/// kept, the same, and panned by a fraction of a pixel. Also with a shadow cast further than a band reaches past its rows: the text
/// above or below a band casting into it is drawn there.
#[test]
fn large_effect_images_drawn_in_bands_look_as_drawn_whole() {
    for (distance, at) in [(1.0, (-100.0, -100.0)), (6.0, (-100.0, -100.0)), (1.0, (-100.3, -100.45))] {
        banded_like_whole(distance, at);
    }
}

/// `at`: where the spread's origin is on the device (at a fraction of a pixel, band edges fall
/// inside rows of pixels).
fn banded_like_whole(distance: f64, at: (f64, f64)) {
    let mut d = doc();
    let lid = d.default_layer();
    let text = "Shadowed text fills the frame. ".repeat(40);
    let (tid, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(10.0, 10.0, 150.0, 150.0), lid, &text, ParaFormat::default()).unwrap();
    let ds = &mut d.item_mut(tid).unwrap().effects.drop_shadow;
    ds.on = true;
    ds.distance = distance;
    ds.size = 0.6;
    ds.opacity = 1.0;
    // 1,400 × 1,400 pixels at 10×: the text's shadow is drawn in several bands, their edges
    // across lines of text.
    let (w, h) = (1400, 1400);
    let view = Affine::translate(at) * Affine::scale(10.0);
    let opts = RenderOptions { background: Some([255, 255, 255, 255]), ..Default::default() };
    let placed = [Placed { spread: SpreadRef::Doc(0), xf: Affine::IDENTITY }];
    let cache = Cache::new();
    let mut banded = Renderer::new();
    banded.threads = 3;
    let first = banded.render(&d, &cache, &placed, w, h, view, &opts);
    let mut one = Renderer::new();
    one.threads = 0;
    let whole = one.render(&d, &cache, &placed, w, h, view, &opts);
    let (w8, m) = (worst(&first, &whole), mean(&first, &whole));
    assert!(w8 <= 8 && m < 0.5, "distance {distance} at {at:?}: max channel difference {w8}, mean {m:.3}");
    assert_eq!(banded.render(&d, &cache, &placed, w, h, view, &opts).pixels, first.pixels, "placed again unchanged");
    // Panned by half a pixel, the bands are placed again across rows of pixels: no seams.
    let panned = Affine::translate((0.0, 0.5)) * view;
    let (kept, whole) = (banded.render(&d, &cache, &placed, w, h, panned, &opts), one.render(&d, &cache, &placed, w, h, panned, &opts));
    let (w8, m) = (worst(&kept, &whole), mean(&kept, &whole));
    assert!(w8 <= 8 && m < 0.5, "distance {distance} at {at:?}, panned: max channel difference {w8}, mean {m:.3}");
}
