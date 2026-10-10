//! `designcraft-cli perf` / `bench`: the performance budgets (plan/architecture.md §9) measured on
//! a synthetic stress document, or on a given file.
//!
//! Each row is the median of several runs. Timings are wall-clock: on a busy machine (load
//! average above ~¾ of the core count) they are noise, and the report says so.
//!
//! Zoom levels follow the canvas: 100% on a 2880×1800 HiDPI canvas is 2 device pixels per point.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use designcraft_color::{Color, Gradient, GradientKind, GradientStop, Swatch, SwatchValue};
use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Asset, AssetId, Content, Document, Fill, Graphic, Item, ItemId, ParaFormat, Shape, SpreadRef, StoryId};
use designcraft_geom::{Affine, Rect, Vec2, shapes};
use designcraft_render::{Placed, RenderOptions, Renderer};

/// Device pixels per point at 100% (HiDPI canvas).
const HIDPI: f64 = 2.0;
const W: u32 = 2880;
const H: u32 = 1800;

/// Deterministic xorshift in 0..1.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % 1_000_000) as f64 / 1_000_000.0
    }
    fn pick<'a>(&mut self, v: &[&'a str]) -> &'a str {
        v[((self.next() * v.len() as f64) as usize).min(v.len() - 1)]
    }
}

const WORDS: &[&str] = &[
    "the",
    "grid",
    "page",
    "margin",
    "column",
    "rhythm",
    "baseline",
    "reader",
    "story",
    "frame",
    "quiet",
    "layout",
    "type",
    "paragraph",
    "composer",
    "weighs",
    "every",
    "line",
    "break",
    "texture",
    "even",
    "hyphen",
    "image",
    "spread",
    "pulse",
    "caption",
    "colour",
    "swatch",
    "ink",
    "tint",
    "gradient",
    "style",
    "voice",
    "printer",
    "proof",
    "bleed",
    "a",
    "of",
    "and",
    "to",
    "in",
    "with",
    "for",
    "is",
    "on",
    "that",
    "by",
    "it",
    "as",
    "careful",
    "measure",
    "leading",
    "kerning",
    "tracking",
    "glyph",
    "serif",
    "italic",
    "heading",
    "running",
    "folio",
];

/// `chars` characters of original filler text in paragraphs of ~400 characters.
fn filler(r: &mut Rng, chars: usize) -> String {
    let mut s = String::with_capacity(chars + 16);
    let mut para = 0;
    while s.len() < chars {
        let w = r.pick(WORDS);
        if para == 0 {
            let mut c = w.chars();
            if let Some(f) = c.next() {
                s.extend(f.to_uppercase());
                s.push_str(c.as_str());
            }
        } else {
            s.push_str(w);
        }
        para += w.len() + 1;
        if para > 400 {
            s.push_str(".\n");
            para = 0;
        } else {
            s.push(' ');
        }
    }
    s
}

/// Procedural RGB image, PNG-encoded.
fn art_png(w: u32, h: u32, seed: f32) -> Vec<u8> {
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let v = ((fx * 9.0 + seed).sin() * (fy * 7.0 - seed).cos() * 0.5 + 0.5).clamp(0.0, 1.0);
            px.extend([(v * 255.0) as u8, ((1.0 - fy) * 200.0) as u8, ((fx * 0.6 + seed * 0.1).fract() * 255.0) as u8, 255]);
        }
    }
    designcraft_render::Rendered { width: w, height: h, pixels: px }.to_png()
}

/// Parameters of the synthetic stress document.
pub struct Spec {
    pub pages: usize,
    pub frames_per_page: usize,
    pub frames_per_story: usize,
    pub chars: usize,
    pub images: usize,
    pub assets: usize,
}

impl Default for Spec {
    fn default() -> Self {
        Spec { pages: 200, frames_per_page: 10, frames_per_story: 50, chars: 1_000_000, images: 500, assets: 25 }
    }
}

/// A `spec.pages`-page facing document: per page a gradient band, a 2×N grid of threaded text
/// frames, and a strip of placed images (one with a drop shadow per spread).
pub fn synthetic(spec: &Spec) -> Result<Document, String> {
    let mut d = Document::new(&NewDocument { pages: spec.pages, ..Default::default() });
    let lid = d.default_layer();
    d.swatches.push(Swatch {
        name: "Perf Gradient".into(),
        value: SwatchValue::Gradient {
            gradient: Gradient {
                kind: GradientKind::Linear,
                stops: vec![
                    GradientStop { offset: 0.0, color: Color::cmyk(0.6, 0.9, 0.3, 0.2), opacity: 1.0, midpoint: 0.5 },
                    GradientStop { offset: 1.0, color: Color::cmyk(0.0, 0.6, 0.8, 0.0), opacity: 1.0, midpoint: 0.5 },
                ],
            },
        },
        locked: false,
        named: true,
        hidden: false,
    });
    let mut rng = Rng(0x5eed);
    let (aw, ah) = (800u32, 600u32);
    let assets: Vec<AssetId> = (0..spec.assets.max(1))
        .map(|k| {
            let id = AssetId(d.alloc());
            let data = art_png(aw, ah, k as f32 * 0.37);
            d.assets.insert(
                id,
                Arc::new(Asset {
                    page: 0,
                    id,
                    name: format!("art-{k}.png"),
                    mime: "image/png".into(),
                    link: None,
                    data: Arc::new(data),
                    pixels: Some((aw, ah)),
                }),
            );
            id
        })
        .collect();
    let total_frames = spec.pages * spec.frames_per_page;
    let stories = total_frames.div_ceil(spec.frames_per_story.max(1));
    let per_story = spec.chars / stories.max(1);
    let rows = spec.frames_per_page.div_ceil(2);
    let (top, strip) = (48.0, 130.0);
    let row_h = (792.0 - 36.0 - top - strip - 8.0) / rows as f64;
    let mut prev: Option<ItemId> = None;
    let mut n_frame = 0usize;
    let mut placed_images = 0usize;
    for abs in 0..spec.pages {
        let Some((si, pi)) = d.page_loc(abs) else { break };
        let sr = SpreadRef::Doc(si);
        let x0 = d.spreads[si].pages[pi].x;
        // Gradient band.
        let id = ItemId(d.alloc());
        let mut band = Item::new(id, lid, Shape::Rectangle, shapes::rectangle(Rect::new(x0 + 36.0, 20.0, x0 + 576.0, 40.0)));
        band.fill = Fill { swatch: "Perf Gradient".into(), ..Fill::none() };
        band.fill.gradient_angle = Some(rng.next() * 90.0);
        d.insert_item(sr, band, None).map_err(|e| e.to_string())?;
        // Text frames.
        for k in 0..spec.frames_per_page {
            let (c, r) = (k % 2, k / 2);
            let fx = x0 + 36.0 + c as f64 * 280.0;
            let fy = top + r as f64 * row_h;
            let rect = Rect::new(fx, fy, fx + 260.0, fy + row_h - 6.0);
            let first = n_frame.is_multiple_of(spec.frames_per_story.max(1));
            let text = if first { filler(&mut rng, per_story) } else { String::new() };
            let (fid, _) = d.add_text_frame(sr, rect, lid, &text, ParaFormat::default()).map_err(|e| e.to_string())?;
            if let (false, Some(p)) = (first, prev) {
                d.thread(p, fid).map_err(|e| e.to_string())?;
            }
            prev = Some(fid);
            n_frame += 1;
        }
        // Images: spread the requested count evenly over the pages.
        let want = (spec.images * (abs + 1)) / spec.pages - placed_images;
        for k in 0..want {
            let w = 540.0 / want as f64;
            let r = Rect::new(x0 + 36.0 + k as f64 * w, 792.0 - 36.0 - strip, x0 + 36.0 + (k + 1) as f64 * w - 8.0, 792.0 - 36.0);
            let asset = assets[(placed_images + k) % assets.len()];
            let id = ItemId(d.alloc());
            let mut it = Item::new(id, lid, Shape::Rectangle, shapes::rectangle(r));
            let (nw, nh) = (aw as f64, ah as f64);
            let s = (r.width() / nw).max(r.height() / nh);
            it.content = Content::Graphic(Graphic {
                asset,
                size: (nw, nh),
                xf: Affine::translate((r.x0 + (r.width() - nw * s) / 2.0, r.y0 + (r.height() - nh * s) / 2.0)) * Affine::scale(s),
                auto_fit: designcraft_doc::Fitting::FillProportionally,
                fit_align: 4,
                crop: [0.0; 4],
                wrap: Default::default(),
            });
            if k == 0 && pi == 0 {
                it.effects.drop_shadow.on = true;
            }
            d.insert_item(sr, it, None).map_err(|e| e.to_string())?;
        }
        placed_images += want;
    }
    Ok(d)
}

fn median_ms(runs: usize, mut f: impl FnMut()) -> f64 {
    let mut v: Vec<f64> = (0..runs.max(1))
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

/// 1-minute load average (macOS / Linux), if available.
fn load_average() -> Option<f64> {
    let out = std::process::Command::new("sysctl").args(["-n", "vm.loadavg"]).output().ok().filter(|o| o.status.success());
    let text = match out {
        Some(o) => String::from_utf8_lossy(&o.stdout).to_string(),
        None => std::fs::read_to_string("/proc/loadavg").ok()?,
    };
    text.split_whitespace().map(|t| t.trim_matches(|c| c == '{' || c == '}')).find_map(|t| t.parse().ok())
}

/// Compose every story into a fresh cache on `threads` threads.
fn compose_all(doc: &Document, threads: usize) -> Arc<Cache> {
    let cache = Arc::new(Cache::new());
    let ids: Vec<StoryId> = doc.stories.keys().copied().collect();
    let next = AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..threads.max(1) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(id) = ids.get(i) else { break };
                    drop(cache.get(doc, *id, None));
                }
            });
        }
    });
    cache
}

/// All spreads stacked vertically, centred on x = 0 (like the canvas).
fn layout(doc: &Document) -> Vec<(Placed, Rect)> {
    let mut y = 0.0;
    doc.spreads
        .iter()
        .enumerate()
        .map(|(i, sp)| {
            let b = sp.bounds();
            let offset = Vec2::new(-b.center().x, y - b.y0);
            y += b.height() + 36.0;
            (Placed { spread: SpreadRef::Doc(i), xf: designcraft_geom::Affine::translate(offset) }, b + offset)
        })
        .collect()
}

/// View transform showing canvas point `c` at the centre of the image at `scale` px/pt.
fn view_at(c: designcraft_geom::Point, scale: f64) -> Affine {
    Affine::translate((W as f64 / 2.0, H as f64 / 2.0)) * Affine::scale(scale) * Affine::translate(-c.to_vec2())
}

struct Row {
    name: String,
    ms: f64,
    budget: Option<f64>,
}

fn row(rows: &mut Vec<Row>, name: impl Into<String>, ms: f64, budget: Option<f64>) {
    let name = name.into();
    eprintln!("  … {name}: {ms:.2} ms");
    rows.push(Row { name, ms, budget });
}

/// Measure `doc`. `runs` = repetitions per row (median).
fn measure(doc: &Document, runs: usize) -> Result<Vec<Row>, String> {
    let mut rows = vec![];
    let cores = std::thread::available_parallelism().map_or(1, |c| c.get());
    let chars: usize = doc.stories.values().map(|s| s.text.len()).sum();
    // Budget: 250 ms per 300k characters (MT), scaled to this document.
    let compose_budget = 250.0 * (chars as f64 / 300_000.0).max(1.0);
    let mut cache = Arc::new(Cache::new());
    row(
        &mut rows,
        format!("compose all stories cold, MT ({} stories, {}k chars)", doc.stories.len(), chars / 1000),
        median_ms(runs.min(3), || cache = compose_all(doc, cores)),
        Some(compose_budget),
    );
    row(&mut rows, "compose all stories cold, 1 thread", median_ms(1, || drop(compose_all(doc, 1))), None);

    let placed = layout(doc);
    let all: Vec<Placed> = placed.iter().map(|p| p.0).collect();
    let mid = placed.len() / 2;
    let centre = placed[mid].1.center();
    let greek = |scale: f64| RenderOptions {
        background: Some([230, 230, 230, 255]),
        page_shadow: true,
        greek_below_px: 3.0 * HIDPI.min(scale.max(1.0)),
        ..Default::default()
    };
    let mut r = Renderer::new();
    // Warm-up: decode images, glyph outlines, compose (already cached).
    let fit = {
        let b = placed[mid].1;
        let s = ((W as f64 - 80.0) / b.width()).min((H as f64 - 80.0) / b.height());
        (s, view_at(centre, s))
    };
    let dump = std::env::var("DESIGNCRAFT_PERF_DUMP").ok();
    let save = |name: &str, img: designcraft_render::Rendered| {
        if let Some(d) = &dump {
            let _ = std::fs::create_dir_all(d);
            let _ = std::fs::write(format!("{d}/{name}.png"), img.to_png());
        }
    };
    save("fit", r.render(doc, &cache, &all, W, H, fit.1, &greek(fit.0)));
    row(
        &mut rows,
        format!("render spread, fit ({:.0}%), {W}×{H}", fit.0 / HIDPI * 100.0),
        median_ms(runs, || drop(r.render(doc, &cache, &all, W, H, fit.1, &greek(fit.0)))),
        Some(16.0),
    );
    for zoom in [0.25, 1.0, 4.0] {
        let s = zoom * HIDPI;
        // At 100% look at the text block of the left page.
        let c = if zoom >= 1.0 { centre - Vec2::new(placed[mid].1.width() / 4.0, 100.0) } else { centre };
        let v = view_at(c, s);
        save(&format!("zoom-{:.0}", zoom * 100.0), r.render(doc, &cache, &all, W, H, v, &greek(s)));
        let budget = if zoom == 1.0 { Some(16.0) } else { None };
        row(
            &mut rows,
            format!("render at {:.0}%, {W}×{H}", zoom * 100.0),
            median_ms(runs, || drop(r.render(doc, &cache, &all, W, H, v, &greek(s)))),
            budget,
        );
    }
    // Panning at 100%: a new view every frame (no pixel cache), same glyph caches.
    let mut dy = 0.0;
    let pan = median_ms(runs, || {
        dy += 37.0;
        let v = view_at(centre + Vec2::new(-200.0, dy - 300.0), HIDPI);
        drop(r.render(doc, &cache, &all, W, H, v, &greek(HIDPI)));
    });
    row(&mut rows, "render at 100% while panning", pan, Some(16.0));
    // Cold renderer (no glyph-path / image caches): first frame after open.
    let cold = median_ms(runs.min(3), || {
        let mut r = Renderer::new();
        drop(r.render(doc, &cache, &all, W, H, view_at(centre, HIDPI), &greek(HIDPI)));
    });
    row(&mut rows, "render at 100%, cold renderer", cold, None);

    // Keystroke: insert a character in the story of the spread's first frame, recompose, repaint.
    if let Some(sid) = doc.spreads[mid].items.iter().find_map(|i| i.text_frame().map(|t| t.story)) {
        let mut d = doc.clone();
        let mut k = 0usize;
        let v = view_at(centre - Vec2::new(placed[mid].1.width() / 4.0, 100.0), HIDPI);
        let ms = median_ms(runs, || {
            if let Some(st) = d.stories.get_mut(&sid) {
                let st = Arc::make_mut(st);
                let at = (st.text.len() / 2 + k).min(st.text.len());
                let at = (0..=at).rev().find(|i| st.text.is_char_boundary(*i)).unwrap_or(0);
                st.text.insert(at, 'x');
                if let Some(run) = st.chars.iter_mut().find(|r| r.len > 0) {
                    run.len += 1;
                }
                st.rev += 1;
                k += 1;
            }
            drop(r.render(&d, &cache, &all, W, H, v, &greek(HIDPI)));
        });
        row(&mut rows, "keystroke: recompose story + full repaint at 100%", ms, None);
        // What the canvas does: repaint only the damaged region (designcraft_render::damage).
        let ms = median_ms(runs, || {
            let old = d.clone();
            if let Some(st) = d.stories.get_mut(&sid) {
                let st = Arc::make_mut(st);
                let at = (st.text.len() / 2 + k).min(st.text.len());
                let at = (0..=at).rev().find(|i| st.text.is_char_boundary(*i)).unwrap_or(0);
                st.text.insert(at, 'x');
                if let Some(run) = st.chars.iter_mut().find(|r| r.len > 0) {
                    run.len += 1;
                }
                st.rev += 1;
                k += 1;
            }
            let screen = designcraft_geom::Rect::new(0.0, 0.0, W as f64, H as f64);
            let mut px: Option<designcraft_geom::Rect> = None;
            for (sr, rect) in designcraft_render::damage::damage_with(&old, &d, Some(&cache)).unwrap_or_default() {
                let m = all.iter().find(|p| p.spread == sr).map_or(designcraft_geom::Affine::IDENTITY, |p| p.xf);
                let r = v.transform_rect_bbox(m.transform_rect_bbox(rect)).intersect(screen);
                if r.width() > 0.0 && r.height() > 0.0 {
                    px = Some(px.map_or(r, |p| p.union(r)));
                }
            }
            if let Some(p) = px {
                let (x0, y0) = (p.x0.floor(), p.y0.floor());
                let (w, h) = ((p.x1.ceil() - x0) as u32, (p.y1.ceil() - y0) as u32);
                drop(r.render(&d, &cache, &all, w.max(1), h.max(1), Affine::translate((-x0, -y0)) * v, &greek(HIDPI)));
            }
        });
        row(&mut rows, "keystroke: recompose story + damage repaint at 100%", ms, Some(8.0));
    }

    let opts = RenderOptions { printing_only: true, ..Default::default() };
    let mut png = 0usize;
    let mut failed: Option<String> = None;
    let ms = median_ms(runs.min(5), || match r.render_page(doc, &cache, doc.first_page_of_spread(mid), 150.0 / 72.0, true, &opts) {
        Some(img) => png = img.to_png().len(),
        None => failed = Some("the middle spread has no page to render".into()),
    });
    row(&mut rows, format!("export page PNG at 150 dpi ({} kB)", png / 1024), ms, None);

    let mut bytes = vec![];
    row(
        &mut rows,
        format!("save .designcraft ({} pages)", doc.page_count()),
        median_ms(runs.min(3), || match designcraft_format::save(doc) {
            Ok(b) => bytes = b,
            Err(e) => failed = Some(format!("save: {e}")),
        }),
        Some(300.0),
    );
    let open_ms = median_ms(runs.min(3), || {
        if let Err(e) = designcraft_format::load(&bytes) {
            failed = Some(format!("open: {e}"));
        }
    });
    row(&mut rows, "open .designcraft", open_ms, Some(300.0));
    let n = doc.page_count().min(100);
    let pdf = designcraft_pdf::PdfOptions { pages: Some((0..n).collect()), ..Default::default() };
    row(
        &mut rows,
        format!("PDF export {n} pages"),
        median_ms(1, || {
            if let Err(e) = designcraft_pdf::export_pdf(doc, &cache, &pdf) {
                failed = Some(format!("PDF export: {e}"));
            }
        }),
        Some(2000.0 * n as f64 / 100.0),
    );
    match failed {
        Some(e) => Err(e),
        None => Ok(rows),
    }
}

fn print(title: &str, rows: &[Row], noisy: bool) -> Result<(), String> {
    outln!("{title}");
    let mut over = 0;
    outln!("  {:<60} {:>11} {:>10}", "", "measured", "budget");
    for r in rows {
        let (b, flag) = match r.budget {
            Some(b) if r.ms > b => {
                over += 1;
                (format!("{b:>7.0} ms"), "OVER")
            }
            Some(b) => (format!("{b:>7.0} ms"), "ok"),
            None => ("      —   ".into(), ""),
        };
        outln!("  {:<60} {:>8.2} ms {b}  {flag}", r.name, r.ms);
    }
    match (over, noisy) {
        (0, _) => Ok(()),
        (_, true) => {
            outln!("{over} over budget, but the machine is busy: re-run when idle.");
            Ok(())
        }
        _ => {
            outln!("{over} budget(s) exceeded");
            Ok(())
        }
    }
}

fn header() -> (usize, bool) {
    let cores = std::thread::available_parallelism().map_or(1, |c| c.get());
    let load = load_average();
    let noisy = load.is_some_and(|l| l > cores as f64 * 0.75);
    outln!(
        "{cores} cores, load average {}, render threads {}",
        load.map_or("?".into(), |l| format!("{l:.1}")),
        designcraft_render::default_threads()
    );
    if noisy {
        outln!("WARNING: the machine is busy (load average above ¾ of the cores); wall-clock timings below are not trustworthy.");
    }
    (cores, noisy)
}

/// `designcraft-cli perf [--pages N] [--frames N] [--chars N] [--images N] [--runs N] [--strict]`.
pub fn perf(args: &[String]) -> Result<(), String> {
    let mut spec = Spec::default();
    let mut runs = 7usize;
    let mut strict = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut num = || it.next().and_then(|v| v.parse::<usize>().ok()).ok_or(format!("{a} needs a number"));
        match a.as_str() {
            "--pages" => spec.pages = num()?.max(1),
            "--frames" => spec.frames_per_page = num()?.max(1),
            "--chars" => spec.chars = num()?,
            "--images" => spec.images = num()?,
            "--runs" => runs = num()?.max(1),
            "--strict" => strict = true,
            other => return Err(format!("unknown perf option `{other}`")),
        }
    }
    let (_, noisy) = header();
    let t = Instant::now();
    let doc = synthetic(&spec)?;
    let frames = spec.pages * spec.frames_per_page;
    outln!(
        "DesignCraft performance budgets — synthetic: {} pages, {frames} text frames in {} threaded stories, {}k chars, {} images ({} assets), {} gradients (built in {:.0} ms)",
        spec.pages,
        doc.stories.len(),
        doc.stories.values().map(|s| s.text.len()).sum::<usize>() / 1000,
        spec.images,
        spec.assets,
        spec.pages,
        t.elapsed().as_secs_f64() * 1000.0
    );
    let rows = measure(&doc, runs)?;
    print("", &rows, noisy)?;
    let over = rows.iter().filter(|r| r.budget.is_some_and(|b| r.ms > b)).count();
    if strict && over > 0 && !noisy { Err(format!("{over} budget(s) exceeded")) } else { Ok(()) }
}

/// `designcraft-cli bench FILE [--runs N]`: the same measurements on one document.
pub fn bench(args: &[String]) -> Result<(), String> {
    let mut file = None;
    let mut runs = 5usize;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--runs" => runs = it.next().and_then(|v| v.parse().ok()).ok_or("--runs needs a number")?,
            f if file.is_none() && !f.starts_with("--") => file = Some(f.to_string()),
            other => return Err(format!("unknown bench option `{other}`")),
        }
    }
    let file = file.ok_or("usage: designcraft-cli bench FILE [--runs N]")?;
    let (_, noisy) = header();
    let mut s = designcraft_engine::Session::new();
    let t = Instant::now();
    s.execute("file.open", &serde_json::json!({"path": file})).map_err(|e| e.to_string())?;
    let open = t.elapsed().as_secs_f64() * 1000.0;
    let doc = s.doc().map_err(|e| e.to_string())?.doc.clone();
    outln!("{file}: {} pages, {} stories, {} assets (opened in {open:.0} ms)", doc.page_count(), doc.stories.len(), doc.assets.len());
    let rows = measure(&doc, runs)?;
    print("", &rows, noisy)
}
