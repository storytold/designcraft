//! The synthetic stress document `designcraft-cli perf` measures, also used by the PDF export
//! benchmark (`examples/pdf_export.rs`).

use std::sync::Arc;

use designcraft_color::{Color, Gradient, GradientKind, GradientStop, Swatch, SwatchValue};
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Asset, AssetId, Content, Document, Fill, Graphic, Item, ItemId, ParaFormat, Shape, SpreadRef};
use designcraft_geom::{Affine, Rect, shapes};

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
                stops: vec![GradientStop::new(0.0, Color::cmyk(0.6, 0.9, 0.3, 0.2)), GradientStop::new(1.0, Color::cmyk(0.0, 0.6, 0.8, 0.0))],
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
                    id,
                    name: format!("art-{k}.png"),
                    mime: "image/png".into(),
                    data: Arc::new(data),
                    pixels: Some((aw, ah)),
                    ..Default::default()
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
                layer_options: None,
                asset,
                size: (nw, nh),
                xf: Affine::translate((r.x0 + (r.width() - nw * s) / 2.0, r.y0 + (r.height() - nh * s) / 2.0)) * Affine::scale(s),
                clip: None,
                auto_fit: designcraft_doc::Fitting::FillProportionally,
                fit_align: 4,
                crop: [0.0; 4],
                wrap: Default::default(),
                fill: Default::default(),
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
