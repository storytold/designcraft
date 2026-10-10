//! The same synthetic overlapping runs must paint correctly on screen and in exported PDF.
use designcraft_color::{Color, Swatch};
use designcraft_compose::{Cache, compose_story};
use designcraft_doc::{CharAttrs, Document, ParaFormat, SpreadRef, build::NewDocument};
use designcraft_geom::{Affine, Rect};
use designcraft_pdf::{PdfOptions, export_pdf};
use designcraft_render::{Placed, RenderOptions, Rendered, Renderer};

fn document(decoration: u8, hide_first: bool, hide_second: bool) -> Document {
    let mut d = Document::new(&NewDocument { width: 240.0, height: 240.0, facing_pages: false, ..Default::default() });
    d.swatches.push(Swatch::color("Cyan rule", Color::rgb(0.0, 1.0, 1.0)));
    let (_, sid) = d
        .add_text_frame(
            SpreadRef::Doc(0),
            Rect::new(0.0, 0.0, 240.0, 180.0),
            d.default_layer(),
            "HI",
            ParaFormat {
                chars: CharAttrs { font_family: Some("Source Sans 3".into()), size: Some(72.0), fill: Some("[Black]".into()), ..Default::default() },
                ..Default::default()
            },
        )
        .unwrap();
    d.story_mut(sid).unwrap().format_chars(0..1, |f| {
        f.over.skew = Some(45.0);
        if hide_first {
            f.over.fill = Some("[None]".into());
        }
    });
    d.story_mut(sid).unwrap().format_chars(1..2, |f| {
        if hide_second {
            f.over.fill = Some("[None]".into());
        }
        f.over.underline = Some(decoration == 1 || decoration == 3);
        f.over.strikethrough = Some(decoration == 2);
        f.over.underline_color = Some(if decoration == 3 { "[None]" } else { "Cyan rule" }.into());
        f.over.strikethrough_color = Some("Cyan rule".into());
        f.over.underline_weight = Some(Some(18.0));
        f.over.strikethrough_weight = Some(Some(18.0));
        f.over.underline_offset = Some(Some(-24.0));
        // Strike offsets are positive above the baseline in the document model.
        f.over.strikethrough_offset = Some(Some(24.0));
    });
    d
}

fn raster(d: &Document, threads: u16) -> Rendered {
    let cache = Cache::new();
    let mut renderer = Renderer::new();
    renderer.threads = threads;
    let placed = [Placed { spread: SpreadRef::Doc(0), xf: Affine::IDENTITY }];
    let options = RenderOptions { paper: false, background: Some([255; 4]), greek_below_px: 0.0, ..Default::default() };
    let first = renderer.render(d, &cache, &placed, 480, 480, Affine::scale(2.0), &options);
    let cached = renderer.render(d, &cache, &placed, 480, 480, Affine::scale(2.0), &options);
    assert_eq!(first.pixels, cached.pixels, "decoration cache hits must retain paint order");
    first
}

fn pdf(d: &Document) -> Rendered {
    let bytes = export_pdf(d, &Cache::new(), &PdfOptions::default()).unwrap();
    let image = designcraft_render::render_pdf_page(&bytes, 0, 480).unwrap();
    assert_eq!((image.width(), image.height()), (480, 480));
    // hayro renders on transparent paper; composite onto the same white backdrop as the canvas.
    let pixels = image
        .data()
        .iter()
        .flat_map(|p| {
            let white = 255 - p.a;
            [p.r.saturating_add(white), p.g.saturating_add(white), p.b.saturating_add(white), 255]
        })
        .collect();
    Rendered { width: 480, height: 480, pixels }
}

fn dark(p: [u8; 4]) -> bool {
    // Process black is color managed (about 52/51/52 with the bundled profile).
    p[0] < 80 && p[1] < 80 && p[2] < 80
}
fn white(p: [u8; 4]) -> bool {
    p[0] > 245 && p[1] > 245 && p[2] > 245
}
fn cyan(p: [u8; 4]) -> bool {
    p[0] < 35 && p[1] > 220 && p[2] > 220
}

fn check_paint_order(render: impl Fn(&Document) -> Rendered) {
    let plain_doc = document(0, false, false);
    let plain = render(&plain_doc);
    let first_only = render(&document(0, false, true));
    let second_only = render(&document(0, true, false));
    let under = render(&document(1, false, false));
    let strike = render(&document(2, false, false));
    assert_eq!(render(&document(3, false, false)).pixels, plain.pixels, "explicit None must draw no rule");
    let sid = *plain_doc.stories.keys().next().unwrap();
    let cs = compose_story(&plain_doc, sid, &Default::default());
    let line = &cs.frames[0].lines[0];
    let g = line.glyphs.iter().find(|g| g.byte == 1).unwrap();
    let rect = Rect::new(g.x, line.baseline - 24.0 - 9.0, g.x + g.adv, line.baseline - 24.0 + 9.0);
    let mut cross_run = 0;
    let mut exposed = 0;
    for y in (rect.y0 * 2.0).ceil() as u32 + 2..(rect.y1 * 2.0).floor() as u32 - 2 {
        for x in (rect.x0 * 2.0).ceil() as u32 + 2..(rect.x1 * 2.0).floor() as u32 - 2 {
            // The first run's skewed H overlaps the SECOND run's rule. A per-run under/glyph
            // loop would still cover these pixels, so the witness is stronger than one run.
            if dark(first_only.pixel(x, y)) && white(second_only.pixel(x, y)) {
                cross_run += 1;
                assert!(dark(under.pixel(x, y)), "underline covered an earlier run at {x},{y}: {:?}", under.pixel(x, y));
                assert!(cyan(strike.pixel(x, y)), "strike must remain above glyphs at {x},{y}");
            }
            if white(plain.pixel(x, y)) {
                exposed += 1;
                assert!(cyan(under.pixel(x, y)), "the visible underline must retain its fill");
            }
        }
    }
    assert!(cross_run > 20, "fixture needs cross-run interior-ink witnesses, got {cross_run}");
    assert!(exposed > 20, "fixture needs visible rule pixels, got {exposed}");
}

#[test]
fn raster_underlines_are_behind_all_runs_and_strikes_remain_above() {
    for threads in [0, 2] {
        check_paint_order(|d| raster(d, threads));
    }
}

#[test]
fn pdf_underlines_are_behind_all_runs_and_strikes_remain_above() {
    check_paint_order(pdf);
}

#[test]
fn decoration_offsets_are_centers_for_explicit_and_automatic_rules() {
    let mut d = document(1, false, false);
    let sid = *d.stories.keys().next().unwrap();
    for (automatic, size) in [(false, 72.0), (true, 28.0)] {
        d.story_mut(sid).unwrap().format_chars(0..2, |f| {
            f.over.size = Some(size);
            f.over.underline = Some(true);
            f.over.strikethrough = Some(true);
            f.over.underline_color = Some(String::new());
            f.over.strikethrough_color = Some(String::new());
            f.over.fill_tint = Some(0.4);
            f.over.underline_tint = Some(0.9);
            f.over.strikethrough_tint = Some(0.8);
            f.over.underline_weight = Some(if automatic { None } else { Some(12.0) });
            f.over.strikethrough_weight = Some(if automatic { None } else { Some(6.0) });
            f.over.underline_offset = Some(if automatic { None } else { Some(-3.0) });
            f.over.strikethrough_offset = Some(if automatic { None } else { Some(8.0) });
        });
        let cs = compose_story(&d, sid, &Default::default());
        for style in &cs.styles {
            let baseline = 84.222687;
            let rules: Vec<_> = style.rules(10.0, 50.0, baseline).collect();
            assert_eq!(rules.len(), 2);
            let expected = if automatic { [(size * 0.12, size / 14.0), (-size * 0.3, size / 14.0)] } else { [(-3.0, 12.0), (-8.0, 6.0)] };
            for ((rule, rect), (offset, weight)) in rules.iter().zip(expected) {
                assert!((rect.center().y - (baseline + offset)).abs() < 1e-9, "offset must locate the center: {rect:?}");
                assert!((rect.height() - weight).abs() < 1e-9);
                assert_eq!(rule.color, style.fill);
                assert_eq!(rule.tint, 0.4);
            }
        }
    }
}
