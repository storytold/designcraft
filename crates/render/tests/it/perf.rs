//! Renderer caching/culling behaviour, and a render-time regression test (`#[ignore]`: wall-clock
//! timings are only meaningful on an idle machine — run with
//! `cargo test --release -p designcraft-render --test it perf:: -- --ignored`).

use std::time::Instant;

use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Document, ParaFormat, SpreadRef};
use designcraft_geom::{Affine, Rect, Vec2};
use designcraft_render::{Placed, RenderOptions, Renderer};

const WORDS: &[&str] = &["grid", "margin", "column", "rhythm", "baseline", "reader", "story", "frame", "quiet", "layout", "type", "texture"];

/// `pages` pages, each with 6 frames of one threaded story.
fn text_doc(pages: usize) -> Document {
    let mut d = Document::new(&NewDocument { pages, ..Default::default() });
    let lid = d.default_layer();
    let mut text = String::new();
    for i in 0..pages * 3000 {
        text.push_str(WORDS[(i * 7 + i / 5) % WORDS.len()]);
        text.push(if i % 70 == 69 { '\n' } else { ' ' });
    }
    let mut prev = None;
    for abs in 0..pages {
        let (si, pi) = d.page_loc(abs).unwrap();
        let x0 = d.spreads[si].pages[pi].x;
        for k in 0..6 {
            let (c, r) = ((k % 2) as f64, (k / 2) as f64);
            let rect = Rect::new(x0 + 36.0 + c * 280.0, 36.0 + r * 240.0, x0 + 296.0 + c * 280.0, 266.0 + r * 240.0);
            let t = if prev.is_none() { text.as_str() } else { "" };
            let (id, _) = d.add_text_frame(SpreadRef::Doc(si), rect, lid, t, ParaFormat::default()).unwrap();
            if let Some(p) = prev {
                d.thread(p, id).unwrap();
            }
            prev = Some(id);
        }
    }
    d
}

fn placed(d: &Document) -> Vec<Placed> {
    let mut y = 0.0;
    (0..d.spreads.len())
        .map(|i| {
            let b = d.spreads[i].bounds();
            let p = Placed { spread: SpreadRef::Doc(i), xf: Affine::translate(Vec2::new(-b.x0, y)) };
            y += b.height() + 36.0;
            p
        })
        .collect()
}

#[test]
fn glyph_paths_are_cached_and_offscreen_text_is_skipped() {
    let d = text_doc(4);
    let cache = Cache::new();
    let all = placed(&d);
    let mut r = Renderer::new();
    r.threads = 0;
    // 200% view of the top-left of the first spread: only a few frames are visible.
    let view = Affine::scale(2.0);
    let opts = RenderOptions::default();
    let a = r.render(&d, &cache, &all, 800, 600, view, &opts);
    let drawn = r.stats.glyphs;
    assert!(drawn > 100, "glyphs drawn: {drawn}");
    assert!(r.cached_frames() <= 4, "only visible frames get glyph paths: {}", r.cached_frames());
    // Same pixels from the cache.
    let b = r.render(&d, &cache, &all, 800, 600, view, &opts);
    assert!(a.pixels == b.pixels);
    // Culling: far fewer glyphs than the visible frames hold.
    let whole = r.render(&d, &cache, &all, 2400, 1600, Affine::scale(1.0), &opts);
    assert!(r.stats.glyphs > drawn * 2, "{} vs {drawn}", r.stats.glyphs);
    drop(whole);
    // A new composition (edited story) gets fresh paths.
    let mut d2 = d.clone();
    let sid = *d2.stories.keys().next().unwrap();
    d2.set_story_text(sid, "Edited").unwrap();
    let c = r.render(&d2, &cache, &all, 800, 600, view, &opts);
    assert!(r.stats.glyphs < 10);
    assert!(c.pixels != a.pixels);
}

#[test]
fn greeked_text_builds_no_glyph_paths() {
    let d = text_doc(2);
    let cache = Cache::new();
    let mut r = Renderer::new();
    r.threads = 0;
    let opts = RenderOptions { greek_below_px: 6.0, ..Default::default() };
    let img = r.render(&d, &cache, &placed(&d), 400, 300, Affine::scale(0.3), &opts);
    assert_eq!(r.stats.glyphs, 0);
    assert_eq!(r.cached_frames(), 0);
    // The grey bars are drawn.
    let grey = (0..img.width).flat_map(|x| (0..img.height).map(move |y| (x, y))).filter(|&(x, y)| img.pixel(x, y)[0] < 240).count();
    assert!(grey > 200, "{grey}");
}

#[test]
#[ignore = "wall-clock budget; run on an idle machine"]
fn render_text_heavy_spread_at_100_percent_within_budget() {
    let d = text_doc(8);
    let cache = Cache::new();
    let all = placed(&d);
    let mut r = Renderer::new();
    // 100% on a HiDPI 2880×1800 canvas = 2 px/pt, centred on the second spread.
    let view = Affine::scale(2.0) * Affine::translate((0.0, -828.0));
    let opts = RenderOptions { greek_below_px: 6.0, ..Default::default() };
    r.render(&d, &cache, &all, 2880, 1800, view, &opts);
    let mut v: Vec<f64> = (0..9)
        .map(|_| {
            let t = Instant::now();
            drop(r.render(&d, &cache, &all, 2880, 1800, view, &opts));
            t.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    v.sort_by(f64::total_cmp);
    let median = v[v.len() / 2];
    eprintln!("render text-heavy spread at 100%, 2880×1800: {median:.2} ms (glyphs {})", r.stats.glyphs);
    // plan/architecture.md §9: < 16 ms.
    assert!(median < 16.0, "{median:.2} ms");
}
