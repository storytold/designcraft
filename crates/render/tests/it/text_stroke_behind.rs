//! Stroke behind fill on text: the fill covers the inner half of a double-width stroke.
use designcraft_color::{Color, swatch::Swatch};
use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Document, ParaFormat, SpreadRef};
use designcraft_geom::Rect;
use designcraft_render::{RenderOptions, Rendered, Renderer};

const WHITE: [u8; 4] = [255, 255, 255, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const WEIGHT: f64 = 12.0;

/// A 200 pt "I" filled with `fill`, stroked red at [`WEIGHT`] (none when `stroke` is false).
fn render(fill: &str, stroke: bool, behind: bool) -> Rendered {
    let mut d = Document::new(&NewDocument { width: 400.0, height: 400.0, facing_pages: false, ..Default::default() });
    d.swatches.push(Swatch::color("Red", Color::rgb8(255, 0, 0)));
    d.swatches.push(Swatch::color("Blue", Color::rgb8(0, 0, 255)));
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(40.0, 0.0, 400.0, 400.0), lid, "I", ParaFormat::default()).unwrap();
    d.story_mut(sid).unwrap().format_chars(0..1, |f| {
        f.over.size = Some(200.0);
        f.over.fill = Some(fill.into());
        f.over.stroke = Some(if stroke { "Red" } else { designcraft_color::swatch::NONE }.into());
        f.over.stroke_weight = Some(WEIGHT);
        f.over.stroke_behind_fill = Some(behind);
    });
    let mut r = Renderer::new();
    r.threads = 0;
    let cache = Cache::new();
    let cs = cache.get(&d, sid, None);
    assert!(!cs.frames[0].lines.is_empty(), "the glyph is set");
    r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions { paper: false, background: Some(WHITE), ..Default::default() }).unwrap()
}

/// The left edge of the stem on a row through the middle of the unstroked glyph.
fn stem_edge() -> (u32, u32) {
    let img = render("Blue", false, false);
    let blue: Vec<(u32, u32)> = (0..img.height).flat_map(|y| (0..img.width).map(move |x| (x, y))).filter(|&(x, y)| img.pixel(x, y) == BLUE).collect();
    assert!(!blue.is_empty(), "the glyph is drawn");
    let y0 = blue.iter().map(|p| p.1).min().unwrap();
    let y1 = blue.iter().map(|p| p.1).max().unwrap();
    let y = (y0 + y1) / 2;
    let x = (0..img.width).find(|&x| img.pixel(x, y) == BLUE).unwrap();
    (x, y)
}

#[test]
fn stroke_behind_fill_shows_only_outside_the_letter() {
    let (x, y) = stem_edge();
    let inner = x + 3; // inside the outline, within the inner half of a centred stroke
    let outer = x - 9; // outside the outline, beyond the outer half of a centred stroke
    let over = render("Blue", true, false);
    assert_eq!(over.pixel(inner, y), RED, "a centred stroke covers the fill's edge");
    assert_eq!(over.pixel(outer, y), WHITE, "a centred stroke reaches half its weight outside");
    let behind = render("Blue", true, true);
    assert_eq!(behind.pixel(inner, y), BLUE, "the fill covers the stroke behind it");
    assert_eq!(behind.pixel(outer, y), RED, "the full stroke weight lies outside the letter");
    assert_eq!(behind.pixel(x - 15, y), WHITE, "and no further");
    let hollow = render(designcraft_color::swatch::NONE, true, true);
    assert_eq!(hollow.pixel(inner, y), WHITE, "with no fill the letter stays empty");
    assert_eq!(hollow.pixel(outer, y), RED);
}
