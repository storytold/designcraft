use std::io::{Cursor, Read};
use std::sync::Arc;

use designcraft_doc::build::NewDocument;
use designcraft_doc::geom::{Affine, PathData, Rect, shapes};
use designcraft_doc::{Article, Asset, AssetId, Content, Document, Graphic, Item, ItemId, ParaFormat, Shape, SpreadRef, StoryId};
use designcraft_epub::{Block, EpubOptions, HtmlOptions, export_epub, export_html, reading_order};

fn text(doc: &mut Document, spread: usize, x: f64, y: f64, value: &str) -> (ItemId, StoryId) {
    doc.add_text_frame(SpreadRef::Doc(spread), Rect::new(x, y, x + 100.0, y + 10.0), doc.default_layer(), value, ParaFormat::default()).unwrap()
}

fn group(doc: &mut Document, spread: usize, ids: &[ItemId], xf: Affine) -> ItemId {
    let children = {
        let sp = doc.spread_mut(SpreadRef::Doc(spread)).unwrap();
        let children: Vec<_> = ids.iter().map(|id| sp.items.iter().find(|it| it.id == *id).unwrap().clone()).collect();
        sp.items.retain(|it| !ids.contains(&it.id));
        children
    };
    let id = ItemId(doc.alloc());
    let mut item = Item::new(id, doc.default_layer(), Shape::Group, PathData::default());
    item.content = Content::Group { items: children };
    item.xf = xf;
    doc.insert_item(SpreadRef::Doc(spread), item, None).unwrap()
}

fn outputs(doc: &Document) -> [String; 2] {
    let html = export_html(doc, &HtmlOptions::default());
    let bytes = export_epub(doc, &EpubOptions::default()).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xhtml = String::new();
    zip.by_name("OEBPS/content.xhtml").unwrap().read_to_string(&mut xhtml).unwrap();
    [html, xhtml]
}

fn assert_output_order(doc: &Document, values: &[&str]) {
    let positions: Vec<_> = ["HTML", "EPUB"]
        .into_iter()
        .zip(outputs(doc))
        .map(|(format, output)| {
            let positions: Vec<_> =
                values.iter().map(|value| output.find(value).unwrap_or_else(|| panic!("{value} missing from {format}"))).collect();
            for value in values {
                assert_eq!(output.matches(value).count(), 1, "{value} should occur once in {format}");
            }
            (format, positions)
        })
        .collect();
    assert!(positions.iter().all(|(_, p)| p.windows(2).all(|p| p[0] < p[1])), "wrong export order for {values:?}: {positions:?}");
}

#[test]
fn translated_group_uses_its_spread_y_position_in_both_exports() {
    let mut doc = Document::new(&NewDocument::default());
    let (child, _) = text(&mut doc, 0, 36.0, 50.0, "Grouped second");
    group(&mut doc, 0, &[child], Affine::translate((0.0, 300.0)));
    text(&mut doc, 0, 36.0, 200.0, "Ungrouped first");
    assert_output_order(&doc, &["Ungrouped first", "Grouped second"]);
}

#[test]
fn translated_group_uses_its_spread_x_position_on_the_same_line() {
    let mut doc = Document::new(&NewDocument::default());
    let (child, _) = text(&mut doc, 0, 10.0, 200.0, "Grouped right");
    group(&mut doc, 0, &[child], Affine::translate((350.0, 0.0)));
    text(&mut doc, 0, 200.0, 200.0, "Ungrouped left");
    assert_output_order(&doc, &["Ungrouped left", "Grouped right"]);
}

#[test]
fn translated_group_uses_the_page_at_its_spread_position() {
    let mut doc = Document::new(&NewDocument { pages: 3, facing_pages: true, ..Default::default() });
    assert_eq!(doc.spreads[1].pages.len(), 2);
    let width = doc.spreads[1].pages[0].width;
    let (child, _) = text(&mut doc, 1, 36.0, 36.0, "Grouped right page");
    group(&mut doc, 1, &[child], Affine::translate((width, 0.0)));
    text(&mut doc, 1, 36.0, 400.0, "Ungrouped left page");
    assert_output_order(&doc, &["Ungrouped left page", "Grouped right page"]);
}

#[test]
fn nested_and_item_rotations_are_combined_before_measuring_graphics() {
    let mut doc = Document::new(&NewDocument::default());
    let aid = AssetId(doc.alloc());
    // Original one-pixel SVG, constructed in the test rather than copied from a fixture.
    doc.assets.insert(
        aid,
        Arc::new(Asset {
            id: aid,
            name: "Grouped image".into(),
            mime: "image/svg+xml".into(),
            link: None,
            data: Arc::new(br#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><rect width="1" height="1" fill="red"/></svg>"#.to_vec()),
            pixels: None,
            page: 0,
            pdf_crop: Default::default(),
            pdf_box: None,
        }),
    );
    let image = ItemId(doc.alloc());
    let mut item = Item::new(image, doc.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 100.0, 10.0)));
    item.content = Content::Graphic(Graphic {
        layer_options: None,
        asset: aid,
        size: (1.0, 1.0),
        xf: Affine::IDENTITY,
        clip: None,
        auto_fit: Default::default(),
        fit_align: 4,
        crop: [0.0; 4],
        wrap: Default::default(),
        fill: Default::default(),
    });
    // The export writes the alt text, not the file name.
    item.alt_text = "Grouped image".into();
    item.xf = Affine::rotate(std::f64::consts::FRAC_PI_4);
    doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
    let inner = group(&mut doc, 0, &[image], Affine::rotate(-std::f64::consts::FRAC_PI_4));
    group(&mut doc, 0, &[inner], Affine::translate((50.0, 100.0)));
    text(&mut doc, 0, 50.0, 75.0, "Text above image");
    // The two rotations cancel: the image's top is y=100, not the y=50 obtained
    // by rotating the already axis-aligned bounds of its rotated local path.
    assert_output_order(&doc, &["Text above image", "Grouped image"]);
}

#[test]
fn a_threaded_story_keeps_the_position_of_its_first_frame() {
    let mut doc = Document::new(&NewDocument::default());
    let (head, sid) = text(&mut doc, 0, 36.0, 50.0, "Threaded story");
    let (tail, _) = text(&mut doc, 0, 36.0, 10.0, "");
    doc.thread(head, tail).unwrap();
    group(&mut doc, 0, &[head], Affine::translate((0.0, 300.0)));
    let (_, first) = text(&mut doc, 0, 36.0, 200.0, "Standalone first");
    assert_output_order(&doc, &["Standalone first", "Threaded story"]);
    assert_eq!(reading_order(&doc), vec![Block::Story(first), Block::Story(sid)]);
}

#[test]
fn articles_keep_their_explicit_order_and_exclusions() {
    let mut doc = Document::new(&NewDocument::default());
    let (a, _) = text(&mut doc, 0, 36.0, 50.0, "Article first");
    let (b, _) = text(&mut doc, 0, 36.0, 30.0, "Article second");
    let grouped = group(&mut doc, 0, &[a, b], Affine::translate((0.0, 300.0)));
    let (c, _) = text(&mut doc, 0, 36.0, 10.0, "Article third");
    let (omitted, _) = text(&mut doc, 0, 36.0, 5.0, "Excluded story");
    doc.articles.push(Article { name: "Unused".into(), items: vec![omitted], export: false });
    doc.articles.push(Article { name: "Chosen".into(), items: vec![grouped, c, grouped], export: true });
    assert_output_order(&doc, &["Article first", "Article second", "Article third"]);
    for output in outputs(&doc) {
        assert!(!output.contains("Excluded story"));
    }
}

#[test]
fn stable_ties_and_the_existing_item_hidden_filter_are_preserved() {
    let mut doc = Document::new(&NewDocument::default());
    let (a, _) = text(&mut doc, 0, 36.0, 100.0, "First tie");
    let (hidden, _) = text(&mut doc, 0, 36.0, 100.0, "Hidden child");
    doc.item_mut(hidden).unwrap().hidden = true;
    group(&mut doc, 0, &[a, hidden], Affine::IDENTITY);
    text(&mut doc, 0, 36.0, 100.0, "Second tie");
    let (hidden, _) = text(&mut doc, 0, 36.0, 50.0, "Hidden root");
    doc.item_mut(hidden).unwrap().hidden = true;
    text(&mut doc, 0, 36.0, 5.0, "");
    let parent = doc.default_layer();
    doc.add_text_frame(SpreadRef::Parent(0), Rect::new(36.0, 0.0, 136.0, 10.0), parent, "Parent text", ParaFormat::default()).unwrap();
    assert_output_order(&doc, &["First tie", "Second tie"]);
    for output in outputs(&doc) {
        for omitted in ["Hidden child", "Hidden root", "Parent text"] {
            assert!(!output.contains(omitted), "{omitted} should remain excluded");
        }
    }
}
