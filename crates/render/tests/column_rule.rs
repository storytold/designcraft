//! Column rules: the bar a text frame draws in each gutter between its columns
//! (Text Frame Options ▸ Column Rule), on screen and in PDF.

use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Document, ParaFormat, SpreadRef};
use designcraft_geom::{Affine, Rect, Vec2};
use designcraft_render::{Placed, RenderOptions, Rendered, Renderer};

const SIZE: u32 = 160;
const WHITE: u32 = 765;

fn doc() -> Document {
    Document::new(&NewDocument { width: 160.0, height: 160.0, facing_pages: false, ..Default::default() })
}

/// A two-column text frame at (20,20)-(140,140) with a 20 pt gutter, so the columns run
/// x = 20..70 and x = 90..140 and the gutter's centre is x = 80.
fn two_columns(rule: bool, weight: f64) -> Document {
    let mut d = doc();
    let lid = d.default_layer();
    let (fid, _sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(20.0, 20.0, 140.0, 140.0), lid, "", ParaFormat::default()).unwrap();
    let o = &mut d.item_mut(fid).unwrap().text_frame_mut().unwrap().options;
    o.columns = 2;
    o.gutter = 20.0;
    o.column_rule = rule;
    o.column_rule_weight = weight;
    d
}

fn render(d: &Document) -> Rendered {
    let mut r = Renderer::new();
    let opts = RenderOptions { background: Some([255, 255, 255, 255]), ..Default::default() };
    r.render(d, &Cache::new(), &[Placed { spread: SpreadRef::Doc(0), xf: Affine::translate(Vec2::ZERO) }], SIZE, SIZE, Affine::IDENTITY, &opts)
}

fn lum(p: [u8; 4]) -> u32 {
    p[0] as u32 + p[1] as u32 + p[2] as u32
}

/// The columns' interior on the row y = 80 (inside the frame, clear of its edges): the x pixels
/// that are not white, i.e. the ones a rule covers.
fn marks(img: &Rendered) -> Vec<u32> {
    (22..138).filter(|x| lum(img.pixel(*x, 80)) < WHITE - 30).collect()
}

#[test]
fn column_rule_draws_a_bar_in_the_gutter() {
    // The frame holds no text, so the only mark in it is the rule: two points wide, centred on
    // the gutter at x = 80.
    let on = render(&two_columns(true, 2.0));
    assert_eq!(marks(&on), vec![79, 80], "a 2 pt rule sits on the gutter's centre");

    // It runs the columns' full height; nothing is drawn past the frame.
    for y in [24, 80, 136] {
        assert!(lum(on.pixel(80, y)) < WHITE - 30, "in the rule at y = {y}: {:?}", on.pixel(80, y));
    }
    assert_eq!(lum(on.pixel(80, 18)), WHITE, "nothing above the frame");
    assert_eq!(lum(on.pixel(80, 144)), WHITE, "nothing below the frame");

    // A heavier rule is wider.
    assert_eq!(marks(&render(&two_columns(true, 12.0))), (74..86).collect::<Vec<u32>>());
    // A zero weight leaves nothing.
    assert_eq!(marks(&render(&two_columns(true, 0.0))), Vec::<u32>::new());
}

#[test]
fn column_rule_off_draws_nothing() {
    assert_eq!(marks(&render(&two_columns(false, 2.0))), Vec::<u32>::new(), "no rule when the option is off");
}

#[test]
fn a_single_column_has_no_gutter_to_rule() {
    let mut d = doc();
    let lid = d.default_layer();
    let (fid, _sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(20.0, 20.0, 140.0, 140.0), lid, "", ParaFormat::default()).unwrap();
    let o = &mut d.item_mut(fid).unwrap().text_frame_mut().unwrap().options;
    o.column_rule = true;
    o.column_rule_weight = 2.0;
    // A lone column has no gutter, so no rule is drawn anywhere in it.
    assert_eq!(marks(&render(&d)), Vec::<u32>::new());
}
