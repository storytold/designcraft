//! Package structure and object mapping tests.

use std::io::Read;
use std::sync::Arc;

use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{
    Asset, AssetId, Content, Document, Fill, Graphic, Item, ItemId, ParaFormat, Shape, SpreadRef, Stroke, StrokeType, TextFrameOptions,
};
use designcraft_geom::{Affine, Rect, shapes};

use crate::{LineBreaks, PptxError, PptxOptions, export_pptx};

fn unzip(bytes: &[u8]) -> zip::ZipArchive<std::io::Cursor<Vec<u8>>> {
    zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap()
}

fn part(z: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>, name: &str) -> String {
    let mut s = String::new();
    z.by_name(name).unwrap_or_else(|_| panic!("missing {name}")).read_to_string(&mut s).unwrap();
    s
}

fn export(d: &Document, opts: &PptxOptions) -> zip::ZipArchive<std::io::Cursor<Vec<u8>>> {
    unzip(&export_pptx(d, &Cache::new(), opts).unwrap().bytes)
}

fn rect_item(d: &mut Document, r: Rect, shape: Shape) -> ItemId {
    let id = ItemId(d.alloc());
    let path = if shape == Shape::Oval { shapes::ellipse(r) } else { shapes::rectangle(r) };
    let mut it = Item::new(id, d.default_layer(), shape, path);
    it.fill = Fill::swatch("[Black]");
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    id
}

fn edit(d: &mut Document, id: ItemId, f: impl FnOnce(&mut Item)) {
    let it = d.spreads.iter_mut().flat_map(|sp| Arc::make_mut(sp).items.iter_mut()).find(|it| it.id == id).unwrap();
    f(Arc::make_mut(it));
}

/// The text of each shape on a slide (runs joined).
fn shape_texts(slide: &str) -> Vec<String> {
    slide
        .split("<p:sp>")
        .skip(1)
        .map(|sp| sp.split("<a:t>").skip(1).filter_map(|t| t.split("</a:t>").next()).collect::<String>())
        .filter(|t| !t.is_empty())
        .collect()
}

/// Every XML part parses.
fn well_formed(z: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>) {
    let names: Vec<String> = z.file_names().map(str::to_string).collect();
    for n in names.iter().filter(|n| n.ends_with(".xml") || n.ends_with(".rels")) {
        let x = part(z, n);
        let mut r = quick_xml::Reader::from_str(&x);
        loop {
            match r.read_event() {
                Ok(quick_xml::events::Event::Eof) => break,
                Ok(_) => {}
                Err(e) => panic!("{n}: {e}"),
            }
        }
    }
}

#[test]
fn package_has_the_parts_powerpoint_needs() {
    let mut d = Document::new(&NewDocument { pages: 2, ..Default::default() });
    let lid = d.default_layer();
    d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 100.0), lid, "Hello & <world>", ParaFormat::default()).unwrap();
    let r = export_pptx(&d, &Cache::new(), &PptxOptions::default()).unwrap();
    assert_eq!(r.slides, 2);
    let mut z = unzip(&r.bytes);
    for p in [
        "[Content_Types].xml",
        "_rels/.rels",
        "ppt/presentation.xml",
        "ppt/_rels/presentation.xml.rels",
        "ppt/slideMasters/slideMaster1.xml",
        "ppt/slideLayouts/slideLayout1.xml",
        "ppt/theme/theme1.xml",
        "ppt/slides/slide1.xml",
        "ppt/slides/slide2.xml",
        "ppt/slides/_rels/slide2.xml.rels",
        "docProps/core.xml",
    ] {
        assert!(z.by_name(p).is_ok(), "{p}");
    }
    well_formed(&mut z);
    let pres = part(&mut z, "ppt/presentation.xml");
    let (w, h) = (d.settings.page_width, d.settings.page_height);
    assert!(pres.contains(&format!("<p:sldSz cx=\"{}\" cy=\"{}\"/>", (w * 12_700.0).round() as i64, (h * 12_700.0).round() as i64)), "{pres}");
    let s1 = part(&mut z, "ppt/slides/slide1.xml");
    assert_eq!(shape_texts(&s1), ["Hello &amp; &lt;world&gt;"]);
}

#[test]
fn shapes_keep_geometry_fill_stroke_and_effects() {
    let mut d = Document::new(&NewDocument::default());
    let a = rect_item(&mut d, Rect::new(72.0, 72.0, 172.0, 122.0), Shape::Rectangle);
    let b = rect_item(&mut d, Rect::new(200.0, 72.0, 300.0, 122.0), Shape::Oval);
    let c = rect_item(&mut d, Rect::new(72.0, 200.0, 172.0, 300.0), Shape::Rectangle);
    edit(&mut d, a, |it| {
        it.stroke = Stroke { swatch: "[Black]".into(), weight: 2.0, kind: StrokeType::Dashed { pattern: vec![4.0, 2.0] }, ..Stroke::default() };
        it.opacity = 0.5;
    });
    edit(&mut d, b, |it| it.effects.drop_shadow.on = true);
    edit(&mut d, c, |it| {
        let ctr = it.bounds().center();
        it.xf = Affine::rotate_about(30f64.to_radians(), ctr);
    });
    let mut z = export(&d, &PptxOptions::default());
    let s = part(&mut z, "ppt/slides/slide1.xml");
    // 72 pt from the page corner, 100 × 50 pt.
    assert!(s.contains("<a:off x=\"914400\" y=\"914400\"/><a:ext cx=\"1270000\" cy=\"635000\"/></a:xfrm><a:prstGeom prst=\"rect\">"), "{s}");
    assert!(s.contains("<a:prstGeom prst=\"ellipse\">"));
    assert!(s.contains("<a:custDash><a:ds d=\"200000\" sp=\"100000\"/></a:custDash>"), "{s}");
    assert!(s.contains("<a:alpha val=\"50000\"/>"));
    assert!(s.contains("<a:outerShdw"));
    assert!(s.contains("rot=\"1800000\""), "{s}");
}

#[test]
fn composed_lines_become_line_breaks_with_the_story_text() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let text = "Every page begins as an empty field of possibility. The grid arrives first: margins that frame the reading area, columns that set the rhythm.";
    let (fid, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(72.0, 72.0, 200.0, 400.0), lid, text, ParaFormat::default()).unwrap();
    let cache = Cache::new();
    let lines = cache.get(&d, sid, None).frame(fid).map_or(0, |f| f.lines.len());
    assert!(lines > 3, "the frame breaks the text into lines");
    let r = export_pptx(&d, &cache, &PptxOptions::default()).unwrap();
    let mut z = unzip(&r.bytes);
    let s = part(&mut z, "ppt/slides/slide1.xml");
    assert_eq!(s.matches("<a:br>").count(), lines - 1, "{s}");
    // The text survives intact (line-end spaces kept, the composer's hyphens shown).
    let joined = shape_texts(&s).concat();
    assert_eq!(joined.replace('-', ""), text);
    assert!(s.contains("<a:spcPct"), "leading as percentage line spacing");
    assert!(s.contains("typeface=\"Source Serif 4\""));
}

#[test]
fn reflow_writes_one_box_with_native_columns() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (fid, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(72.0, 72.0, 500.0, 300.0), lid, "One\nTwo", ParaFormat::default()).unwrap();
    edit(&mut d, fid, |it| {
        if let Content::Text(t) = &mut it.content {
            t.options = TextFrameOptions { columns: 2, gutter: 12.0, ..t.options.clone() };
        }
    });
    let mut z = export(&d, &PptxOptions { line_breaks: LineBreaks::Reflow, ..Default::default() });
    let s = part(&mut z, "ppt/slides/slide1.xml");
    assert!(s.contains("numCol=\"2\" spcCol=\"152400\""), "{s}");
    assert!(!s.contains("<a:br>"));
    assert_eq!(s.matches("<a:p>").count(), 2);
}

#[test]
fn threaded_story_splits_at_its_frames() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let text = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega";
    let (f1, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(72.0, 72.0, 160.0, 110.0), lid, text, ParaFormat::default()).unwrap();
    let f2 = rect_item(&mut d, Rect::new(300.0, 72.0, 388.0, 400.0), Shape::Rectangle);
    edit(&mut d, f2, |it| {
        it.fill = Fill::none();
        it.content = Content::Text(designcraft_doc::TextFrame { story: sid, options: TextFrameOptions::default() });
    });
    if let Some(st) = d.stories.get_mut(&sid) {
        Arc::make_mut(st).frames = vec![f1, f2];
    }
    let mut z = export(&d, &PptxOptions::default());
    let s = part(&mut z, "ppt/slides/slide1.xml");
    // Two text shapes, the story starting in the first and ending in the second.
    let texts: Vec<String> = shape_texts(&s).iter().map(|t| t.replace('-', "")).collect();
    assert_eq!(texts.len(), 2, "{s}");
    assert!(texts[0].starts_with("alpha") && !texts[0].contains("omega"), "{texts:?}");
    assert!(texts[1].trim_end().ends_with("omega"), "{texts:?}");
    assert_eq!(texts.concat().trim_end(), text);
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(w, h, image::Rgba([200, 30, 30, 255]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

#[test]
fn pictures_are_cropped_to_their_frames() {
    let mut d = Document::new(&NewDocument::default());
    let asset = AssetId(d.alloc());
    d.assets.insert(
        asset,
        Arc::new(Asset {
            id: asset,
            name: "red.png".into(),
            mime: "image/png".into(),
            link: None,
            data: Arc::new(png(200, 100)),
            pixels: Some((200, 100)),
            page: 0,
        }),
    );
    let id = rect_item(&mut d, Rect::new(100.0, 100.0, 200.0, 200.0), Shape::Rectangle);
    edit(&mut d, id, |it| {
        it.fill = Fill::none();
        // 200 × 100 image at 1:1, its left quarter cut off by the frame.
        it.content = Content::Graphic(Graphic {
            asset,
            size: (200.0, 100.0),
            xf: Affine::translate((50.0, 100.0)),
            auto_fit: designcraft_doc::Fitting::None,
            fit_align: 0,
            crop: [0.0; 4],
        });
    });
    let mut z = export(&d, &PptxOptions::default());
    assert!(z.by_name("ppt/media/image1.png").is_ok());
    let s = part(&mut z, "ppt/slides/slide1.xml");
    assert!(s.contains("<a:srcRect l=\"25000\" t=\"0\" r=\"25000\" b=\"0\"/>"), "{s}");
    let rels = part(&mut z, "ppt/slides/_rels/slide1.xml.rels");
    assert!(rels.contains("../media/image1.png"));
}

#[test]
fn bad_pages_and_hostile_geometry_are_errors_not_crashes() {
    let mut d = Document::new(&NewDocument::default());
    let r = export_pptx(&d, &Cache::new(), &PptxOptions { pages: Some(vec![5]), ..Default::default() });
    assert!(matches!(r, Err(PptxError::BadPage(6))));
    assert!(matches!(export_pptx(&d, &Cache::new(), &PptxOptions { pages: Some(vec![]), ..Default::default() }), Err(PptxError::NoPages)));
    let a = rect_item(&mut d, Rect::new(10.0, 10.0, 10.0, 10.0), Shape::Rectangle);
    let b = rect_item(&mut d, Rect::new(10.0, 10.0, 50.0, 50.0), Shape::Rectangle);
    edit(&mut d, a, |it| it.xf = Affine::new([0.0, 0.0, 0.0, 0.0, 0.0, 0.0]));
    edit(&mut d, b, |it| it.xf = Affine::new([f64::NAN, 0.0, 0.0, 1e300, f64::INFINITY, 0.0]));
    let lid = d.default_layer();
    let (t, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(10.0, 10.0, 60.0, 60.0), lid, "x", ParaFormat::default()).unwrap();
    edit(&mut d, t, |it| it.xf = Affine::new([1.0, 0.0, 3.0, 1.0, 0.0, 0.0]));
    let mut z = export(&d, &PptxOptions::default());
    well_formed(&mut z);
}

#[test]
fn fonts_embed_as_embedded_opentype() {
    let face = designcraft_fonts::FontDb::global().face("Source Serif 4", "Regular");
    let eot = crate::fonts::eot(&face).unwrap();
    let le32 = |at: usize| u32::from_le_bytes([eot[at], eot[at + 1], eot[at + 2], eot[at + 3]]);
    assert_eq!(le32(0) as usize, eot.len());
    assert_eq!(le32(4) as usize, face.data().len());
    assert_eq!(le32(8), 0x0002_0001);
    assert_eq!(u16::from_le_bytes([eot[34], eot[35]]), 0x504C, "magic");
    assert!(eot.ends_with(face.data()));

    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 100.0), lid, "Embedded", ParaFormat::default()).unwrap();
    let mut z = export(&d, &PptxOptions { embed_fonts: true, ..Default::default() });
    let pres = part(&mut z, "ppt/presentation.xml");
    assert!(
        pres.contains("embedTrueTypeFonts=\"1\"") && pres.contains("<p:embeddedFont><p:font typeface=\"Source Serif 4\"/><p:regular r:id="),
        "{pres}"
    );
    assert!(z.by_name("ppt/fonts/font1.fntdata").is_ok());
    let ct = part(&mut z, "[Content_Types].xml");
    assert!(ct.contains("application/x-fontdata"));
}
