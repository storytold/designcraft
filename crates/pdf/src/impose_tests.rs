//! Export Imposed PDF: where copies, their bleed and the crop marks land in the file.

use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Document, Fill, Item, ItemId, SpreadRef};
use designcraft_geom::impose::{CropMarks, Rotation, Spec};
use designcraft_geom::{Rect, Size};

use crate::*;

/// A one-page 100 × 60 pt document with a 9 pt bleed, its page filled black well past the bleed.
fn card() -> Document {
    let mut d = Document::new(&NewDocument { pages: 1, width: 100.0, height: 60.0, facing_pages: false, ..NewDocument::default() });
    d.settings.bleed = [9.0; 4];
    let lid = d.default_layer();
    let id = ItemId(d.alloc());
    let mut b = Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(-40.0, -40.0, 140.0, 100.0)));
    b.fill = Fill::swatch("[Black]");
    d.insert_item(SpreadRef::Doc(0), b, None).unwrap();
    d
}

/// Sheet 300 × 200, 20 pt margins, 12 pt gaps (less than twice the bleed), 2 × 2 copies.
fn opts(copies: usize, marks: CropMarks) -> ImposeOptions {
    ImposeOptions {
        spec: Spec { sheet: Size::new(300.0, 200.0), margins: [20.0; 4], gap: [12.0, 12.0], rotation: Rotation::Off, center: false },
        copies,
        bleed: true,
        crop_marks: marks,
        ..ImposeOptions::default()
    }
}

/// What page `index` paints, in sheet coordinates (y down from the top): each fill's area inside
/// the clips in force, and each stroked line.
fn painted(bytes: &[u8], index: usize, height: f64) -> (Vec<kurbo::Rect>, Vec<kurbo::Line>) {
    use hayro_interpret::font::Glyph;
    use hayro_interpret::{
        BlendMode, ClipPath, Context, Device, GlyphDrawMode, Image, InterpreterCache, InterpreterSettings, Paint, PathDrawMode, SoftMask,
        interpret_page,
    };
    use kurbo::Shape as _;
    #[derive(Default)]
    struct Rec {
        clips: Vec<kurbo::Rect>,
        fills: Vec<kurbo::Rect>,
        lines: Vec<kurbo::Line>,
    }
    impl Device<'_> for Rec {
        fn set_soft_mask(&mut self, _: Option<SoftMask<'_>>) {}
        fn set_blend_mode(&mut self, _: BlendMode) {}
        fn draw_path(&mut self, p: &kurbo::BezPath, xf: kurbo::Affine, _: &Paint<'_>, mode: &PathDrawMode) {
            let p = xf * p.clone();
            match mode {
                PathDrawMode::Fill(_) => {
                    let area = self.clips.iter().fold(p.bounding_box(), |a, c| a.intersect(*c));
                    self.fills.push(area);
                }
                PathDrawMode::Stroke(_) => {
                    let mut at = None;
                    for el in p.elements() {
                        match *el {
                            kurbo::PathEl::MoveTo(a) => at = Some(a),
                            kurbo::PathEl::LineTo(b) => {
                                if let Some(a) = at {
                                    self.lines.push(kurbo::Line::new(a, b));
                                }
                                at = Some(b);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        fn push_clip_path(&mut self, c: &ClipPath) {
            self.clips.push(c.path.bounding_box());
        }
        fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'_>>, _: BlendMode) {}
        fn draw_glyph(&mut self, _: &Glyph<'_>, _: kurbo::Affine, _: kurbo::Affine, _: &Paint<'_>, _: &GlyphDrawMode) {}
        fn draw_image(&mut self, _: Image<'_, '_>, _: kurbo::Affine) {}
        fn pop_clip_path(&mut self) {
            self.clips.pop();
        }
        fn pop_transparency_group(&mut self) {}
    }
    let pdf = craft_pdf::syntax::Pdf::new(bytes.to_vec()).expect("parse");
    let cache = InterpreterCache::new();
    let big = kurbo::Rect::new(-1e6, -1e6, 1e6, 1e6);
    let mut ctx = Context::new(kurbo::Affine::IDENTITY, big, &cache, pdf.xref(), InterpreterSettings::default());
    let mut rec = Rec::default();
    interpret_page(&pdf.pages()[index], &mut ctx, &mut rec);
    // PDF space has y up from the bottom.
    let flip = kurbo::Affine::new([1.0, 0.0, 0.0, -1.0, 0.0, height]);
    let round = |v: f64| (v * 1000.0).round() / 1000.0;
    let fills =
        rec.fills.iter().map(|r| flip.transform_rect_bbox(*r)).map(|r| Rect::new(round(r.x0), round(r.y0), round(r.x1), round(r.y1))).collect();
    let lines = rec
        .lines
        .iter()
        .map(|l| {
            let (a, b) = (flip * l.p0, flip * l.p1);
            kurbo::Line::new((round(a.x), round(a.y)), (round(b.x), round(b.y)))
        })
        .collect();
    (fills, lines)
}

#[test]
fn copies_keep_their_bleed_up_to_half_the_gap() {
    let d = card();
    let r = export_imposed(&d, &Cache::new(), &opts(5, CropMarks::None)).unwrap();
    assert_eq!(r.pages, 2, "4 a sheet, 5 copies");
    let pdf = craft_pdf::syntax::Pdf::new(r.bytes.clone()).unwrap();
    assert_eq!(pdf.pages().len(), 2);
    let (fills, lines) = painted(&r.bytes, 0, 200.0);
    assert!(lines.is_empty(), "no marks: {lines:?}");
    // Trims: (20, 20)–(120, 80), (132, 20)–(232, 80), (20, 92)–(120, 152), (132, 92)–(232, 152).
    // The 9 pt bleed shows towards the margins, and only 6 pt (half the gap) between copies.
    for want in [
        Rect::new(11.0, 11.0, 126.0, 86.0),
        Rect::new(126.0, 11.0, 241.0, 86.0),
        Rect::new(11.0, 86.0, 126.0, 161.0),
        Rect::new(126.0, 86.0, 241.0, 161.0),
    ] {
        assert!(fills.contains(&want), "{want:?} in {fills:?}");
    }
    assert_eq!(fills.len(), 4, "{fills:?}");
    // The fifth copy starts the second sheet.
    let (fills, _) = painted(&r.bytes, 1, 200.0);
    assert_eq!(fills, [Rect::new(11.0, 11.0, 126.0, 86.0)]);
    // Without bleed, each copy is clipped at its trim.
    let r = export_imposed(&d, &Cache::new(), &ImposeOptions { bleed: false, ..opts(1, CropMarks::None) }).unwrap();
    assert_eq!(painted(&r.bytes, 0, 200.0).0, [Rect::new(20.0, 20.0, 120.0, 80.0)]);
}

#[test]
fn crop_marks_land_at_the_trim_lines() {
    let d = card();
    let r = export_imposed(&d, &Cache::new(), &opts(4, CropMarks::Corners)).unwrap();
    let (_, lines) = painted(&r.bytes, 0, 200.0);
    // The 12 pt gap leaves no room between copies; into the margins each mark starts outside the
    // 9 pt bleed and stops at the sheet edge.
    assert!(lines.contains(&kurbo::Line::new((20.0, 11.0), (20.0, 0.0))), "{lines:?}");
    assert!(lines.contains(&kurbo::Line::new((11.0, 20.0), (0.0, 20.0))), "{lines:?}");
    assert!(lines.contains(&kurbo::Line::new((241.0, 20.0), (259.0, 20.0))), "{lines:?}");
    assert!(lines.contains(&kurbo::Line::new((232.0, 161.0), (232.0, 179.0))), "{lines:?}");
    // Each copy has two margin sides with two corners each: four marks a copy.
    assert_eq!(lines.len(), 4 * 4, "{lines:?}");
    assert!(lines.iter().all(|l| [l.p0.x, l.p1.x].iter().all(|x| !(120.0..=132.0).contains(x) || l.p0.x == l.p1.x)), "none in the gap: {lines:?}");
    // Border: one mark above and below at each of the four x trim lines, left and right at the four
    // y trim lines.
    let r = export_imposed(&d, &Cache::new(), &opts(4, CropMarks::Border)).unwrap();
    let (_, lines) = painted(&r.bytes, 0, 200.0);
    assert_eq!(lines.len(), 16, "{lines:?}");
    assert!(lines.contains(&kurbo::Line::new((132.0, 11.0), (132.0, 0.0))), "{lines:?}");
    assert!(lines.contains(&kurbo::Line::new((241.0, 92.0), (259.0, 92.0))), "{lines:?}");
}

#[test]
fn rotated_copies_turn_clockwise() {
    let mut d = card();
    // A 10 pt black bar along the page's top edge.
    let lid = d.default_layer();
    let id = ItemId(d.alloc());
    let mut bar = Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, 100.0, 10.0)));
    bar.fill = Fill::swatch("[Black]");
    d.insert_item(SpreadRef::Doc(0), bar, None).unwrap();
    let o = ImposeOptions {
        spec: Spec { sheet: Size::new(60.0, 100.0), margins: [0.0; 4], gap: [0.0; 2], rotation: Rotation::Auto, center: true },
        ..ImposeOptions::default()
    };
    let im = impose(&d, &o).unwrap();
    assert!(im.grid.rotated && im.grid.per_sheet() == 1, "{:?}", im.grid);
    let r = export_imposed(&d, &Cache::new(), &o).unwrap();
    let (fills, _) = painted(&r.bytes, 0, 100.0);
    // The page's top edge is the copy's right edge.
    assert!(fills.contains(&Rect::new(50.0, 0.0, 60.0, 100.0)), "{fills:?}");
}

#[test]
fn impose_refuses_what_cannot_be_laid_out() {
    let d = card();
    let small = ImposeOptions { spec: Spec { sheet: Size::new(50.0, 50.0), ..opts(1, CropMarks::None).spec }, ..opts(1, CropMarks::None) };
    assert!(matches!(impose(&d, &small), Err(PdfError::Impose(e)) if e.contains("doesn't fit")));
    assert_eq!(impose(&d, &ImposeOptions { pages: Some(vec![3]), ..opts(1, CropMarks::None) }), Err(PdfError::BadPage(4)));
    assert_eq!(impose(&d, &ImposeOptions { pages: Some(vec![]), ..opts(1, CropMarks::None) }), Err(PdfError::NoPages));
    assert!(matches!(impose(&d, &opts(MAX_COPIES + 1, CropMarks::None)), Err(PdfError::Impose(_))));
    assert_eq!(impose(&d, &opts(8, CropMarks::None)).unwrap().sheets, 2);
}
