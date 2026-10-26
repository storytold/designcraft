use std::io::Write;

use designcraft_color::swatch;
use designcraft_doc::{Content, Document, Item, Leading, PAGE_NUMBER, PageSide, Shape, Story};
use designcraft_geom::{Point, Rect};

use super::*;

fn import(sla: &str) -> Imported {
    import_sla_report(sla.as_bytes(), &|_| None, &|_| false).unwrap()
}

fn items(d: &Document, spread: usize) -> Vec<&Item> {
    d.spreads[spread].items.iter().map(|i| i.as_ref()).collect()
}

fn story_of<'a>(d: &'a Document, item: &Item) -> &'a Story {
    let Content::Text(tf) = &item.content else { panic!("not a text frame") };
    &d.stories[&tf.story]
}

fn spread_bounds(item: &Item) -> Rect {
    item.xf.transform_rect_bbox(designcraft_geom::Shape::bounding_box(&item.path.to_bezpath()))
}

/// Standard base64 with padding.
pub(crate) fn base64(bytes: &[u8]) -> String {
    const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    bytes
        .chunks(3)
        .flat_map(|c| {
            let n = c.iter().enumerate().fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
            (0..4).map(move |i| if i <= c.len() { B64[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' })
        })
        .collect()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// A Scribus 1.7 document: two facing pages side by side on the canvas (one spread), a master
/// page, two layers, colours, styles, a threaded story, an image, shapes and a group.
const DOC_17: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<SCRIBUSUTF8NEW Version="1.7.2">
<Document PageWidth="300" PageHeight="400" PagePositioning="1" FirstPageNumber="5" UnitIndex="1" BleedTop="9" DocumentInfoTitle="Flyer">
  <Color Name="Black" Space="CMYK" C="0" M="0" Y="0" K="100"/>
  <Color Name="White" Space="CMYK" C="0" M="0" Y="0" K="0"/>
  <Color Name="Sky" Space="RGB" R="0" G="128" B="255"/>
  <Color Name="Gold" Space="CMYK" C="0" M="20" Y="100" K="0" Spot="1"/>
  <Color Name="Reg" Space="CMYK" C="100" M="100" Y="100" K="100" Register="1"/>
  <CharacterStyle Name="Default Character Style" DefaultStyle="1" Font="Liberation Serif Regular" FontSize="10" FontColor="Black"/>
  <CharacterStyle Name="Strong" CParent="Default Character Style" Font="Liberation Serif Bold" FontColor="Gold"/>
  <ParagraphStyle Name="Default Paragraph Style" DefaultStyle="1" Alignment="0" LineSpacingMode="0" LineSpacing="13"/>
  <ParagraphStyle Name="Heading" Parent="Default Paragraph Style" Alignment="1" LineSpacingMode="0" LineSpacing="24" GapAfter="6" FontSize="20">
    <Tabs Type="1" Pos="100" Fill="."/>
  </ParagraphStyle>
  <ParagraphStyle Name="Sub" Parent="Heading" Alignment="2"/>
  <Layers Number="0" Level="0" Name="Back" IsViewable="1" IsPrintable="1" IsEditable="1" LayerColor="#ff0000"/>
  <Layers Number="1" Level="1" Name="Front" IsViewable="0" IsPrintable="0" IsEditable="0" LayerColor="#00ff00"/>
  <MasterPage PageXPosition="100" PageYPosition="20" PageWidth="300" PageHeight="400" BorderLeft="10" BorderRight="20" BorderTop="30" BorderBottom="40" PageName="Normal" LeftPage="0"/>
  <Page PageXPosition="100" PageYPosition="20" PageWidth="300" PageHeight="400" BorderLeft="10" BorderRight="20" BorderTop="30" BorderBottom="40" PageNumber="0" MasterPageName="Normal" LeftPage="1" VerticalGuides="50 60" HorizontalGuides="70"/>
  <Page PageXPosition="400" PageYPosition="20" PageWidth="300" PageHeight="400" BorderLeft="10" BorderRight="20" BorderTop="30" BorderBottom="40" PageNumber="1" MasterPageName="Normal" LeftPage="0"/>
  <Page PageXPosition="100" PageYPosition="460" PageWidth="300" PageHeight="400" PageNumber="2" MasterPageName="" LeftPage="1"/>
  <Sections><Section Number="0" Name="0" From="0" To="2" Type="Type_i_ii_iii" Start="3" Active="1"/></Sections>
  <MasterObject XPosition="110" YPosition="380" OwnPage="0" ItemID="900" ItemType="4" Width="50" Height="20" OnMasterPage="Normal" Layer="0">
    <StoryText><DefaultStyle/><var name="pgno"/><trail/></StoryText>
  </MasterObject>
  <PageObject XPosition="110" YPosition="50" OwnPage="0" ItemID="1" ItemType="4" Width="100" Height="50" Layer="0" Columns="2" ColumnGap="8" TextToFrameDistanceLeft="1" TextToFrameDistanceTop="2" TextToFrameDistanceBottom="3" TextToFrameDistanceRight="4" VerticalAlignment="1" NextItem="2" BackItem="-1">
    <StoryText>
      <DefaultStyle Parent="Default Paragraph Style"/>
      <Content Chars="Title"/>
      <tab/>
      <Content CParent="Strong" Chars="Bold" FontSize="14"/>
      <para Parent="Heading"/>
      <Content Chars="Line"/>
      <breakline/>
      <Content Chars="two"/>
      <nbspace/>
      <Content Chars="end"/>
      <para Parent="Sub" LineSpacingMode="0" LineSpacing="30"/>
      <Content Chars="Last"/>
      <trail/>
    </StoryText>
  </PageObject>
  <PageObject XPosition="420" YPosition="50" OwnPage="1" ItemID="2" ItemType="4" Width="100" Height="50" Layer="1" NextItem="-1" BackItem="1">
    <StoryText><DefaultStyle/><trail/></StoryText>
  </PageObject>
  <PageObject XPosition="150" YPosition="200" OwnPage="0" ItemID="3" ItemType="2" Width="80" Height="60" Rotation="90" FillColor="Sky" FillShade="50" LineColor="Black" LineWidth="2" ImageScaleX="0.5" ImageScaleY="0.25" ImageOffsetX="10" ImageOffsetY="20" ImageFileName="images/photo.svg" Layer="0"/>
  <PageObject XPosition="450" YPosition="300" OwnPage="-1" ItemID="4" ItemType="6" Width="10" Height="10" FrameType="3" FillColor="FromSVG#336699" Path="M0 0 L10 0 L5 10 Z" Layer="0"/>
  <PageObject XPosition="120" YPosition="300" OwnPage="0" ItemID="5" ItemType="5" Width="100" Height="1" LineColor="Black" LineWidth="0.5" Layer="0"/>
  <PageObject XPosition="200" YPosition="100" OwnPage="0" ItemID="6" ItemType="12" Width="40" Height="40" Layer="0">
    <PageObject XPosition="210" YPosition="110" OwnPage="0" ItemID="7" ItemType="6" Width="10" Height="10" FrameType="1" FillColor="Black" Layer="0"/>
    <PageObject XPosition="220" YPosition="120" OwnPage="0" ItemID="8" ItemType="6" Width="10" Height="10" FrameType="0" FillColor="Missing Colour" Layer="0"/>
  </PageObject>
  <PageObject XPosition="200" YPosition="100" OwnPage="0" ItemID="9" ItemType="16" Width="40" Height="40" Layer="0"/>
  <PageObject XPosition="110" YPosition="500" OwnPage="2" ItemID="10" ItemType="4" Width="100" Height="50" Layer="0">
    <StoryText><DefaultStyle/><Content Chars="Own story"/><trail/></StoryText>
  </PageObject>
</Document>
</SCRIBUSUTF8NEW>"##;

#[test]
fn scribus_documents_are_recognised() {
    assert!(is_sla(br#"<?xml version="1.0" encoding="UTF-8"?><SCRIBUSUTF8NEW Version="1.7.2">"#));
    assert!(is_sla("\u{feff}<!-- note --><?xml version=\"1.0\"?>\n<SCRIBUSUTF8NEW Version=\"1.5.8\">".as_bytes()));
    assert!(is_sla(b"<SCRIBUS Version=\"1.2\">"));
    assert!(!is_sla(b"<SCRIBUSX>"));
    assert!(!is_sla(b"<?xml version=\"1.0\"?><svg/>"));
    assert!(!is_sla(b"<?xml unterminated"));
    assert!(!is_sla(b""));
    let err = import_sla(b"<html/>", &|_| None).map(|_| ()).unwrap_err();
    assert!(matches!(err, SlaError::NotSla(_)), "{err}");
}

#[test]
fn pages_side_by_side_form_spreads_with_their_masters() {
    let im = import(DOC_17);
    let d = &im.document;
    assert_eq!(d.title, "Flyer");
    assert!(d.settings.facing_pages);
    assert_eq!((d.settings.page_width, d.settings.page_height), (300.0, 400.0));
    assert_eq!(d.settings.bleed[0], 9.0);
    assert_eq!(d.settings.horizontal_units, designcraft_geom::Unit::Millimeters);
    // Pages 1 and 2 share a canvas row; page 3 is below them.
    assert_eq!(d.spreads.len(), 2);
    let (left, right) = (&d.spreads[0].pages[0], &d.spreads[0].pages[1]);
    assert_eq!((left.x, right.x), (0.0, 300.0));
    assert_eq!((left.side, right.side), (PageSide::Left, PageSide::Right));
    // A left page's inside is its right edge.
    assert_eq!((left.margins.inside, left.margins.outside, left.margins.top), (20.0, 10.0, 30.0));
    assert_eq!((right.margins.inside, right.margins.outside), (10.0, 20.0));
    let master = &d.parents[0];
    assert_eq!(master.parent.as_ref().map(|p| (p.prefix.as_str(), p.name.as_str())), Some(("A", "Normal")));
    assert_eq!((left.parent, right.parent), (Some(master.id), Some(master.id)));
    assert_eq!(d.spreads[1].pages[0].parent, None, "an empty master name applies no master");
    // Guides are page-relative in Scribus, spread coordinates here.
    let guides: Vec<f64> = left.guides.iter().map(|g| g.position).collect();
    assert_eq!(guides, vec![50.0, 60.0, 70.0]);
    // The master's page-number frame sits on the parent, relative to the master page.
    let m = &master.items[0];
    assert_eq!(m.xf.translation(), (10.0, 360.0).into());
    assert!(story_of(d, m).text.starts_with(PAGE_NUMBER));
    assert_eq!(d.sections[0].start_number, Some(3));
    assert_eq!(d.sections[0].style, designcraft_doc::NumberStyle::LowerRoman);
}

#[test]
fn layers_keep_their_order_visibility_and_items() {
    let d = import(DOC_17).document;
    let names: Vec<&str> = d.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, vec!["Front", "Back"], "the frontmost layer is listed first");
    let front = &d.layers[0];
    assert_eq!((front.visible, front.printable, front.locked, front.color), (false, false, true, [0, 255, 0]));
    // Items on the back layer come before the front layer's in the spread.
    let first = items(&d, 0);
    let last = first.last().unwrap();
    assert_eq!(last.layer, front.id);
}

#[test]
fn colours_become_swatches() {
    let im = import(DOC_17);
    let d = &im.document;
    let sw = |name: &str| d.swatches.iter().find(|s| s.name == name);
    assert!(sw("Black").is_none() && sw("White").is_none(), "Black and White are DesignCraft's own");
    assert!(matches!(sw("Sky").map(|s| &s.value), Some(swatch::SwatchValue::Color { color: designcraft_color::Color::Rgb { .. }, .. })));
    assert!(matches!(sw("Gold").map(|s| &s.value), Some(swatch::SwatchValue::Color { color_type: designcraft_color::ColorType::Spot, .. })));
    assert!(sw("Reg").is_none(), "a registration colour is [Registration]");
    assert!(sw("FromSVG#336699").is_some(), "a colour named after its value is made");
    assert!(im.warnings.iter().any(|w| w.contains("Missing Colour")), "{:?}", im.warnings);
    let image = items(d, 0).into_iter().find(|i| matches!(i.content, Content::Graphic(_))).unwrap();
    assert_eq!((image.fill.swatch.as_str(), image.fill.tint), ("Sky", 0.5));
    assert_eq!((image.stroke.swatch.as_str(), image.stroke.weight), (swatch::BLACK, 2.0));
}

#[test]
fn styles_carry_their_parents_and_attributes() {
    let d = import(DOC_17).document;
    let st = &d.styles;
    let basic = st.paragraph.iter().find(|s| s.name == designcraft_doc::BASIC_PARAGRAPH).unwrap();
    // The default paragraph and character styles are [Basic Paragraph].
    assert_eq!(basic.chars.font_family.as_deref(), Some("Liberation Serif"));
    assert_eq!(basic.chars.size, Some(10.0));
    assert_eq!(basic.chars.leading, Some(Leading::Points(13.0)));
    assert_eq!(basic.para.hyphenate, Some(false), "Scribus hyphenates only where the text has soft hyphens");
    let heading = st.paragraph.iter().find(|s| s.name == "Heading").unwrap();
    assert_eq!(heading.based_on.as_deref(), Some(designcraft_doc::BASIC_PARAGRAPH));
    assert_eq!(heading.para.align, Some(designcraft_doc::Align::Center));
    assert_eq!(heading.para.space_after, Some(6.0));
    assert_eq!((heading.chars.size, heading.chars.leading), (Some(20.0), Some(Leading::Points(24.0))));
    let tab = &heading.para.tabs.as_ref().unwrap()[0];
    assert_eq!((tab.position, tab.align, tab.leader.as_str()), (100.0, designcraft_doc::TabAlign::Right, "."));
    let sub = st.paragraph.iter().find(|s| s.name == "Sub").unwrap();
    assert_eq!(sub.based_on.as_deref(), Some("Heading"));
    let strong = st.character.iter().find(|s| s.name == "Strong").unwrap();
    assert_eq!(strong.based_on, None, "the default character style is no style");
    assert_eq!((strong.chars.font_style.as_deref(), strong.chars.fill.as_deref()), (Some("Bold"), Some("Gold")));
}

#[test]
fn threaded_frames_share_one_story_with_its_paragraphs_and_runs() {
    let d = import(DOC_17).document;
    let first = items(&d, 0).into_iter().find(|i| matches!(&i.content, Content::Text(tf) if tf.options.columns == 2)).unwrap();
    let story = story_of(&d, first);
    assert_eq!(story.text, "Title\tBold\nLine\u{2028}two\u{a0}end\nLast");
    assert_eq!(story.frames.len(), 2, "the second frame continues the thread");
    let styles: Vec<&str> = story.paras.iter().map(|p| p.style.as_str()).collect();
    // A `para` styles the paragraph before it; `trail` the last one.
    assert_eq!(styles, vec!["Heading", "Sub", designcraft_doc::BASIC_PARAGRAPH]);
    // The bold run: its character style and its own size.
    let mut at = 0;
    let bold = story
        .chars
        .iter()
        .find(|r| {
            let hit = story.text.get(at..at + r.len) == Some("Bold");
            at += r.len;
            hit
        })
        .unwrap();
    assert_eq!((bold.format.style.as_str(), bold.format.over.size), ("Strong", Some(14.0)));
    // A paragraph's own line spacing goes onto its runs.
    let lead: Vec<Option<Leading>> = story.chars.iter().map(|r| r.format.over.leading).collect();
    assert!(lead.contains(&Some(Leading::Points(30.0))), "{lead:?}");
    let Content::Text(tf) = &first.content else { panic!("unexpected content") };
    assert_eq!((tf.options.gutter, tf.options.inset), (8.0, [2.0, 1.0, 3.0, 4.0]));
    assert_eq!(tf.options.vertical_justification, designcraft_doc::VerticalJustification::Center);
    // The unthreaded frame on page 3 has its own story.
    let own = &d.spreads[1].items[0];
    assert_eq!(story_of(&d, own).text, "Own story");
    d.check().unwrap();
}

#[test]
fn items_are_placed_rotated_and_shaped() {
    let im = import(DOC_17);
    let d = &im.document;
    let all = items(d, 0);
    // The image: at its page offset, rotated 90° clockwise about its top-left.
    let image = all.iter().find(|i| matches!(i.content, Content::Graphic(_))).unwrap();
    assert_eq!(image.xf.translation(), (50.0, 180.0).into());
    let corner = image.xf * Point::new(80.0, 0.0);
    assert!(near(corner.x, 50.0) && near(corner.y, 260.0), "{corner:?}");
    let Content::Graphic(g) = &image.content else { panic!("unexpected content") };
    // One image pixel to the point, scaled, offset in image units.
    assert_eq!(g.xf.as_coeffs(), [0.5, 0.0, 0.0, 0.25, 5.0, 5.0]);
    let asset = &d.assets[&g.asset];
    assert_eq!((asset.name.as_str(), asset.link.as_deref()), ("photo.svg", Some("images/photo.svg")));
    assert!(im.warnings.iter().any(|w| w.contains("not found") && w.contains("images/photo.svg")), "{:?}", im.warnings);
    // The polygon (no owning page): on the page it lies on, shaped by its path.
    let polygon = all.iter().find(|i| i.fill.swatch == "FromSVG#336699").unwrap();
    assert_eq!((polygon.shape, polygon.xf.translation()), (Shape::Path, (350.0, 280.0).into()));
    assert_eq!(polygon.path.subpaths[0].anchors.len(), 3);
    // The line: a 100 pt stroke, no fill.
    let line = all.iter().find(|i| i.shape == Shape::GraphicLine).unwrap();
    assert!(line.fill.is_none());
    assert_eq!(spread_bounds(line).width(), 100.0);
    // The group: its members keep their places on the page.
    let group = all.iter().find(|i| i.shape == Shape::Group).unwrap();
    let Content::Group { items: members } = &group.content else { panic!("unexpected content") };
    let placed: Vec<(f64, f64)> = members.iter().map(|m| (group.xf * m.xf).translation()).map(|v| (v.x, v.y)).collect();
    assert_eq!(placed, vec![(110.0, 90.0), (120.0, 100.0)]);
    assert_eq!(members[0].shape, Shape::Oval);
    // The table isn't imported, and says so.
    assert!(im.warnings.iter().any(|w| w.contains("1 tables")), "{:?}", im.warnings);
}

/// The same kinds of content in Scribus 1.5's attribute names.
const DOC_15: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<SCRIBUSUTF8NEW Version="1.5.8">
<DOCUMENT ANZPAGES="1" PAGEWIDTH="200" PAGEHEIGHT="100" BOOK="0" FIRSTNUM="1" UNITS="0" TITLE="">
  <COLOR NAME="Black" SPACE="CMYK" C="0" M="0" Y="0" K="100"/>
  <COLOR NAME="Old Red" CMYK="#00ffff00"/>
  <COLOR NAME="Old Blue" RGB="#0000ff"/>
  <STYLE NAME="Default Paragraph Style" DefaultStyle="1" ALIGN="0" LINESPMode="1" LINESP="15"/>
  <STYLE NAME="Body" PARENT="Default Paragraph Style" ALIGN="3" INDENT="5" FIRST="-5" VOR="2" NACH="3" DROP="1" DROPLIN="3" FONT="Times New Roman Regular" FONTSIZE="9"/>
  <CHARSTYLE CNAME="Default Character Style" DefaultStyle="1" FONT="Arial Regular" FONTSIZE="12" FCOLOR="Black"/>
  <LAYERS NUMMER="0" LEVEL="0" NAME="Hintergrund" SICHTBAR="1" DRUCKEN="1" EDIT="1" LAYERC="#000000"/>
  <MASTERPAGE PAGEXPOS="100" PAGEYPOS="20" PAGEWIDTH="200" PAGEHEIGHT="100" NUM="0" NAM="Normal" MNAM="" LEFT="0"/>
  <PAGE PAGEXPOS="100" PAGEYPOS="20" PAGEWIDTH="200" PAGEHEIGHT="100" BORDERLEFT="5" BORDERRIGHT="6" BORDERTOP="7" BORDERBOTTOM="8" NUM="0" NAM="" MNAM="Normal" LEFT="0"/>
  <PAGEOBJECT XPOS="110" YPOS="30" OwnPage="0" ItemID="11" PTYPE="4" WIDTH="80" HEIGHT="40" PCOLOR="Old Red" SHADE="100" PCOLOR2="Old Blue" PWIDTH="1.5" COLUMNS="1" COLGAP="0" EXTRA="1" TEXTRA="2" BEXTRA="3" REXTRA="4" LAYER="0" NEXTITEM="-1" BACKITEM="-1" ROT="270" path="M0 0 L80 0 L80 40 L0 40 Z">
    <StoryText>
      <DefaultStyle ALIGN="1" LINESP="15" FONTSIZE="39"/>
      <ITEXT FONTSIZE="20" FEATURES="underline smallcaps" KERN="5" CH="Old"/>
      <para PARENT="Body"/>
      <ITEXT CH="Style"/>
      <trail ALIGN="2"/>
    </StoryText>
  </PAGEOBJECT>
  <PAGEOBJECT XPOS="150" YPOS="50" OwnPage="0" ItemID="12" PTYPE="2" WIDTH="20" HEIGHT="20" LOCALSCX="2" LOCALSCY="2" LOCALX="0" LOCALY="0" PFILE="pic.png" LAYER="0" NEXTITEM="-1" BACKITEM="-1"/>
</DOCUMENT>
</SCRIBUSUTF8NEW>"##;

#[test]
fn scribus_15_attribute_names() {
    let mut png = Vec::new();
    image::RgbaImage::new(4, 3).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let read = |p: &str| (p == "pic.png").then(|| png.clone());
    let im = import_sla_report(DOC_15.as_bytes(), &read, &|f| f == "Times New Roman").unwrap();
    let d = &im.document;
    assert_eq!(d.title, "Untitled");
    assert!(!d.settings.facing_pages);
    assert_eq!(d.spreads[0].pages[0].side, PageSide::Single);
    assert_eq!(d.layers[0].name, "Hintergrund");
    let frame = &d.spreads[0].items[0];
    assert_eq!((frame.fill.swatch.as_str(), frame.stroke.swatch.as_str(), frame.stroke.weight), ("Old Red", "Old Blue", 1.5));
    // 270° clockwise: the frame's right edge points up.
    let p = frame.xf * Point::new(80.0, 0.0);
    assert!(near(p.x, 10.0) && near(p.y, -70.0), "{p:?}");
    let story = story_of(d, frame);
    assert_eq!(story.text, "Old\nStyle");
    assert_eq!(story.paras[0].style, "Body");
    assert_eq!(story.paras[1].para.align, Some(designcraft_doc::Align::Right));
    let first = &story.chars[0].format.over;
    assert_eq!((first.size, first.underline, first.tracking), (Some(20.0), Some(true), Some(50.0)));
    assert_eq!(first.capitalization, Some(designcraft_doc::Capitalization::SmallCaps));
    // The story default's size reaches runs that don't set one.
    assert_eq!(story.chars.last().unwrap().format.over.size, Some(39.0));
    let body = d.styles.paragraph.iter().find(|s| s.name == "Body").unwrap();
    assert_eq!(
        (body.para.align, body.para.left_indent, body.para.first_line_indent),
        (Some(designcraft_doc::Align::LeftJustified), Some(5.0), Some(-5.0))
    );
    assert_eq!((body.para.drop_cap_lines, body.para.drop_cap_chars), (Some(3), Some(1)));
    // The family the fonts know ends the name; "Roman" isn't taken for a style.
    assert_eq!((body.chars.font_family.as_deref(), body.chars.font_style.as_deref()), (Some("Times New Roman"), Some("Regular")));
    let basic = d.styles.paragraph.iter().find(|s| s.name == designcraft_doc::BASIC_PARAGRAPH).unwrap();
    assert_eq!((basic.chars.font_family.as_deref(), basic.chars.leading), (Some("Arial"), Some(Leading::Auto)));
    let image = &d.spreads[0].items[1];
    let Content::Graphic(g) = &image.content else { panic!("not an image") };
    assert_eq!((g.size, d.assets[&g.asset].pixels), ((4.0, 3.0), Some((4, 3))));
    assert!(im.warnings.is_empty(), "{:?}", im.warnings);
}

#[test]
fn font_names_split_into_family_and_style() {
    let none = |_: &str| false;
    let split = |n: &str| text::split_font(n, &none);
    assert_eq!(split("Liberation Serif Bold Italic"), ("Liberation Serif".into(), "Bold Italic".into()));
    assert_eq!(split("Times New Roman Regular"), ("Times New Roman".into(), "Regular".into()));
    assert_eq!(split("Arial Black Regular"), ("Arial Black".into(), "Regular".into()));
    assert_eq!(split("Futura"), ("Futura".into(), "Regular".into()));
    assert_eq!(split("Bold"), ("Bold".into(), "Regular".into()));
    assert_eq!(split(""), ("".into(), "Regular".into()));
    let known = |f: &str| f == "Helvetica Light";
    assert_eq!(text::split_font("Helvetica Light Oblique", &known), ("Helvetica Light".into(), "Oblique".into()));
}

#[test]
fn embedded_images_are_inflated() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="6"/>"#;
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(svg).unwrap();
    let mut q = (svg.len() as u32).to_be_bytes().to_vec();
    q.extend(z.finish().unwrap());
    let b64 = base64(&q);
    let sla = DOC_15.replace(r#"PFILE="pic.png""#, &format!(r#"ImageData="{b64}" inlineImageExt="svg""#));
    let d = import(&sla).document;
    let Content::Graphic(g) = &d.spreads[0].items[1].content else { panic!("not an image") };
    let asset = &d.assets[&g.asset];
    assert_eq!((asset.data.as_slice(), asset.mime.as_str(), asset.link.as_deref()), (&svg[..], "image/svg+xml", None));
    // Broken embedded data is a warning, not a failure.
    let broken = DOC_15.replace(r#"PFILE="pic.png""#, r#"ImageData="!!!""#);
    assert!(import(&broken).warnings.iter().any(|w| w.contains("embedded image")));
}

#[test]
fn hostile_documents_are_errors_or_warnings_never_panics() {
    let wrap =
        |body: &str| format!(r#"<SCRIBUSUTF8NEW Version="1.7.2"><Document PageWidth="NaN" PageHeight="-5">{body}</Document></SCRIBUSUTF8NEW>"#);
    let page = r#"<Page PageXPosition="0" PageYPosition="0" PageWidth="1e300" PageHeight="inf" PageNumber="0"/>"#;
    // No pages, no document element, broken XML.
    assert!(import_sla(wrap("").as_bytes(), &|_| None).is_err());
    assert!(import_sla(br#"<SCRIBUSUTF8NEW Version="1"></SCRIBUSUTF8NEW>"#, &|_| None).is_err());
    assert!(import_sla(br#"<SCRIBUSUTF8NEW Version="1"><Document>"#, &|_| None).is_err());
    // A thread that loops back on itself, duplicate ids, a frame threaded to nothing, garbage
    // numbers and paths, a group with no members and deeply nested groups.
    let mut nested = String::new();
    for _ in 0..100 {
        nested.push_str(r#"<PageObject ItemType="12" XPosition="0" YPosition="0">"#);
    }
    nested.push_str(r#"<PageObject ItemType="6" Width="1" Height="1"/>"#);
    nested.push_str(&"</PageObject>".repeat(100));
    let body = format!(
        r#"{page}
        <PageObject ItemID="1" ItemType="4" NextItem="2" BackItem="2" XPosition="NaN" Width="-3" Height="1e400" Rotation="inf"><StoryText><Content Chars="A"/><para/><breakframe/><trail/></StoryText></PageObject>
        <PageObject ItemID="2" ItemType="4" NextItem="1" BackItem="1"><StoryText><Content Chars="B"/></StoryText></PageObject>
        <PageObject ItemID="2" ItemType="4" NextItem="99"/>
        <PageObject ItemType="4"><ITEXT CH="no id"/></PageObject>
        <PageObject ItemType="6" Path="M 0 0 L nonsense"/>
        <PageObject ItemType="12"/>
        <PageObject ItemType="2" ImageFileName="" ImageScaleX="0"/>
        <PageObject ItemType="99" OwnPage="12345"/>
        <Color Name="X" Space="CMYK"/>
        {nested}"#
    );
    let im = import(&wrap(&body));
    im.document.check().unwrap();
    for s in im.document.stories.values() {
        s.check().unwrap();
    }
    assert!(im.warnings.iter().any(|w| w.contains("nested too deeply")), "{:?}", im.warnings);
}

#[test]
fn items_without_a_page_stop_scanning_pages_past_the_lookup_budget() {
    // 1000 pages × 10 100 items without `OwnPage`: one past the budget, which goes on page 1.
    let mut body = String::new();
    for n in 0..1000 {
        body.push_str(&format!(r#"<Page PageXPosition="{}" PageYPosition="0" PageWidth="10" PageHeight="10" PageNumber="{n}"/>"#, n * 20));
    }
    for _ in 0..10_100 {
        body.push_str(r#"<PageObject ItemType="6" XPosition="19985" YPosition="5" Width="1" Height="1"/>"#);
    }
    let im = import(&format!(r#"<SCRIBUSUTF8NEW Version="1.7.2"><Document>{body}</Document></SCRIBUSUTF8NEW>"#));
    im.document.check().unwrap();
    assert!(im.warnings.iter().any(|w| w.starts_with("100 items without a page of their own")), "{:?}", im.warnings);
    assert_eq!(im.document.spreads.iter().map(|s| s.items.len()).sum::<usize>(), 10_100);
    assert_eq!(items(&im.document, 0).len(), 100);
}

#[test]
fn shadowed_text_casts_a_hard_drop_shadow_from_its_frame() {
    // A spine title: a filled frame turned 90°, all its text shadowed (5 % right and down of
    // 20 pt text: 1 pt each way, which the turn makes 1 pt left and 1 pt down on the page).
    // A frame whose text is only partly shadowed keeps no shadow, with a warning; one whose
    // runs are shadowed through a character style casts the style's offset and colour, at the
    // style's size; one whose size comes from its paragraph style is offset by that size.
    let im = import(
        r##"<SCRIBUSUTF8NEW Version="1.7.2"><Document>
  <Color Name="Black" Space="CMYK" C="0" M="0" Y="0" K="100"/>
  <Color Name="Red" Space="RGB" R="200" G="40" B="20"/>
  <Color Name="Sky" Space="RGB" R="0" G="128" B="255"/>
  <CharacterStyle Name="Default Character Style" DefaultStyle="1" FontSize="12" Features="inherit" StrokeColor="Black" StrokeShade="100" TextShadowXOffset="5" TextShadowYOffset="-5"/>
  <CharacterStyle Name="Glow" FontSize="10" Features="inherit shadowed" StrokeColor="Sky" StrokeShade="50" TextShadowXOffset="10" TextShadowYOffset="0"/>
  <ParagraphStyle Name="Title" FontSize="30"/>
  <Page PageXPosition="0" PageYPosition="0" PageWidth="300" PageHeight="400" PageNumber="0"/>
  <PageObject XPosition="50" YPosition="10" OwnPage="0" ItemID="1" ItemType="4" Width="300" Height="30" Rotation="90" FillColor="Red" LineColor="Black" LineWidth="2">
    <StoryText><DefaultStyle FontSize="20"/><Content Features="inherit shadowed" Chars="Spine title"/><trail/></StoryText>
  </PageObject>
  <PageObject XPosition="100" YPosition="10" OwnPage="0" ItemID="2" ItemType="4" Width="100" Height="30">
    <StoryText><DefaultStyle/><Content Features="inherit shadowed" Chars="Half"/><Content Chars=" plain"/><trail/></StoryText>
  </PageObject>
  <PageObject XPosition="100" YPosition="50" OwnPage="0" ItemID="3" ItemType="4" Width="100" Height="30">
    <StoryText><DefaultStyle/><Content CParent="Glow" Chars="Styled"/><trail/></StoryText>
  </PageObject>
  <PageObject XPosition="100" YPosition="90" OwnPage="0" ItemID="4" ItemType="4" Width="100" Height="30">
    <StoryText><DefaultStyle/><Content Features="inherit shadowed" Chars="Title"/><para Parent="Title"/><trail/></StoryText>
  </PageObject>
</Document></SCRIBUSUTF8NEW>"##,
    );
    im.document.check().unwrap();
    let it = items(&im.document, 0);
    assert_eq!(it.len(), 5, "the spine frame's fill and stroke go to a rectangle of their own");

    let (back, spine) = (it[0], it[1]);
    assert!(matches!(back.content, Content::Unassigned));
    assert_eq!((back.fill.swatch.as_str(), back.stroke.weight), ("Red", 2.0));
    assert_eq!(back.xf, spine.xf);
    assert!(spine.fill.is_none() && spine.stroke.is_none());
    let ds = &spine.effects.drop_shadow;
    assert!(ds.on && ds.size == 0.0 && ds.opacity == 1.0 && ds.color == swatch::BLACK, "{ds:?}");
    // The renderer's offset for a light angle: (-cos, sin) × distance.
    let a = ds.angle.to_radians();
    let off = (-a.cos() * ds.distance, a.sin() * ds.distance);
    assert!(near(off.0, -1.0) && near(off.1, 1.0), "{off:?}");

    assert!(!it[2].effects.drop_shadow.on);
    assert!(im.warnings.iter().any(|w| w.starts_with("text shadows on part of a story")), "{:?}", im.warnings);

    let ds = &it[3].effects.drop_shadow;
    assert!(ds.on && ds.color == "Sky" && near(f64::from(ds.opacity), 0.5), "{ds:?}");
    let a = ds.angle.to_radians();
    assert!(near(-a.cos() * ds.distance, 1.0) && near(a.sin() * ds.distance, 0.0), "{ds:?}");

    let ds = &it[4].effects.drop_shadow;
    let a = ds.angle.to_radians();
    assert!(near(-a.cos() * ds.distance, 1.5) && near(a.sin() * ds.distance, 1.5), "{ds:?}");
}
