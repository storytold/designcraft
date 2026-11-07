//! Import/export tests against small synthetic documents shaped like the XML Scribus writes.

use crate::*;

const FIXTURE: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<SCRIBUSUTF8NEW Version="1.7.3">
  <Document PageCount="2" PageWidth="300" PageHeight="400" BorderLeft="10" BorderRight="20" BorderTop="30" BorderBottom="40"
            PageOrientation="0" PageSize="Custom" FirstPageNumber="1" PagePositioning="1" UnitIndex="1"
            BaselineGridSpacing="12" BaselineGridOffset="6" ActiveLayer="0"
            DocumentInfoTitle="Fixture" DocumentInfoAuthor="Tester" DocumentInfoKeywords="one, two">
    <Color Name="Ink" Space="CMYK" C="0" M="100" Y="100" K="0"/>
    <Color Name="Mist" Space="RGB" R="200" G="220" B="240"/>
    <Hyphenator/>
    <CharacterStyle Name="Default Character Style" DefaultStyle="1" Font="Test Font Regular" FontSize="12" FontColor="Black" ScaleHorizontal="100" ScaleVertical="100" BaselineOffset="0" Kerning="0"/>
    <CharacterStyle Name="cBig" Font="Test Font Bold" FontSize="18" FontColor="Ink" FillShade="50" ScaleHorizontal="90" ScaleVertical="80" BaselineOffset="25" Kerning="120"/>
    <ParagraphStyle Name="Default Paragraph Style" DefaultStyle="1" Alignment="0" LineSpacing="14" LeftMargin="0" RightMargin="0" FirstIndent="0" GapBefore="0" GapAfter="0" HasDropCap="0"/>
    <ParagraphStyle Name="pBody" Alignment="3" LineSpacing="16" LeftMargin="4" RightMargin="5" FirstIndent="6" GapBefore="7" GapAfter="8" HasDropCap="1" DropCapLines="3" CParent="cBig" Language="it"/>
    <ParagraphStyle Name="pForced" Alignment="4" LineSpacing="16" Language="it"/>
    <ParagraphStyle Name="pAuto" LineSpacingMode="1" LineSpacing="16" Language="it"/>
    <ParagraphStyle Name="pHead" Alignment="1" LineSpacing="20" LeftMargin="0" RightMargin="0" FirstIndent="0" GapBefore="0" GapAfter="12" HasDropCap="0"/>
    <Layers Number="0" Level="0" Name="Main" IsViewable="1" IsPrintable="1" IsEditable="1" LayerColor="#ff0000"/>
    <Layers Number="1" Level="1" Name="Top" IsViewable="1" IsPrintable="1" IsEditable="1" LayerColor="#00ff00"/>
    <Sections>
      <Section Number="0" Name="Front" From="0" To="1" Type="Type_1_2_3" Start="1" Reversed="0" Active="1"/>
    </Sections>
    <MasterPage PageXPosition="100" PageYPosition="20" PageWidth="300" PageHeight="400" BorderLeft="10" BorderRight="20" BorderTop="30" BorderBottom="40"
                PageNumber="0" PageName="Master A" LeftPage="0" VerticalGuides="" HorizontalGuides=""/>
    <Page PageXPosition="100" PageYPosition="20" PageWidth="300" PageHeight="400" BorderLeft="10" BorderRight="20" BorderTop="30" BorderBottom="40"
          PageNumber="1" PageName="" MasterPageName="Master A" LeftPage="1" VerticalGuides="50" HorizontalGuides="60"/>
    <Page PageXPosition="400" PageYPosition="20" PageWidth="300" PageHeight="400" BorderLeft="10" BorderRight="20" BorderTop="30" BorderBottom="40"
          PageNumber="2" PageName="" MasterPageName="Master A" LeftPage="0" VerticalGuides="" HorizontalGuides=""/>
    <MasterObject XPosition="120" YPosition="40" OwnPage="0" ItemID="10" ItemType="4" Width="100" Height="20" FrameType="0"
                  AutoName="mfoot" Columns="1" ColumnGap="0" TextToFrameDistanceLeft="0" TextToFrameDistanceTop="0"
                  TextToFrameDistanceBottom="0" TextToFrameDistanceRight="0" VerticalAlignment="0" TextPathType="0" TextPathFlipped="0"
                  Path="M0 0 L100 0 L100 20 L0 20 L0 0 Z" ContourLinePath="M0 0 L100 0 L100 20 L0 20 L0 0 Z"
                  OnMasterPage="Master A" Layer="0" NextItem="-1" BackItem="-1">
      <StoryText>
        <DefaultStyle/>
        <Content Chars="Master "/>
        <var name="pgno"/>
        <trail/>
      </StoryText>
    </MasterObject>
    <PageObject XPosition="120" YPosition="40" OwnPage="1" ItemID="11" ItemType="4" Width="200" Height="100" FrameType="0"
                AutoName="body" Rotation="0" Columns="2" ColumnGap="12" TextToFrameDistanceLeft="5" TextToFrameDistanceTop="6"
                TextToFrameDistanceBottom="7" TextToFrameDistanceRight="8" VerticalAlignment="1" TextPathType="0" TextPathFlipped="0"
                Path="M0 0 L200 0 L200 100 L0 100 L0 0 Z" ContourLinePath="M0 0 L200 0 L200 100 L0 100 L0 0 Z"
                Layer="0" NextItem="12" BackItem="-1">
      <StoryText>
        <DefaultStyle Parent="pBody"/>
        <Content Chars="Hello"/>
        <para Parent="pHead"/>
        <Content CParent="cBig" Chars="World" FontSize="20"/>
        <trail Parent="pBody"/>
      </StoryText>
    </PageObject>
    <PageObject XPosition="140" YPosition="180" OwnPage="1" ItemID="12" ItemType="4" Width="200" Height="100" FrameType="0"
                AutoName="body2" Rotation="0" Columns="1" ColumnGap="0" TextToFrameDistanceLeft="0" TextToFrameDistanceTop="0"
                TextToFrameDistanceBottom="0" TextToFrameDistanceRight="0" VerticalAlignment="0" TextPathType="0" TextPathFlipped="0"
                Path="M0 0 L200 0 L200 100 L0 100 L0 0 Z" ContourLinePath="M0 0 L200 0 L200 100 L0 100 L0 0 Z"
                Layer="0" NextItem="-1" BackItem="11"/>
    <PageObject XPosition="430" YPosition="40" OwnPage="2" ItemID="13" ItemType="6" Width="80" Height="50" FrameType="0"
                AutoName="rot" Rotation="30" FillColor="Mist" FillShade="75" LineColor="Ink" LineWidth="2.5" LinePenStyle="1"
                Path="M0 0 L80 0 L80 50 L0 50 L0 0 Z" ContourLinePath="M0 0 L80 0 L80 50 L0 50 L0 0 Z" Layer="0"/>
    <PageObject XPosition="430" YPosition="200" OwnPage="2" ItemID="14" ItemType="2" Width="80" Height="50" FrameType="0"
                AutoName="pic" ImageScaleX="0.5" ImageScaleY="0.25" ImageOffsetX="2" ImageOffsetY="-4" ImageRotation="0"
                ImageFileName="pic.png" Path="M0 0 L80 0 L80 50 L0 50 L0 0 Z" ContourLinePath="M0 0 L80 0 L80 50 L0 50 L0 0 Z" Layer="0"/>
    <PageObject XPosition="120" YPosition="300" OwnPage="1" ItemID="15" ItemType="5" Width="120" Height="1" FrameType="0"
                AutoName="line" LineColor="Ink" LineWidth="1" LinePenStyle="1" Path="M0 0 L120 0" ContourLinePath="M0 0 L120 0" Layer="0"/>
    <PageObject XPosition="120" YPosition="330" OwnPage="1" ItemID="16" ItemType="12" Width="60" Height="30" FrameType="0"
                AutoName="Group" Path="M0 0 L60 0 L60 30 L0 30 L0 0 Z" ContourLinePath="M0 0 L60 0 L60 30 L0 30 L0 0 Z" Layer="0">
      <PageObject XPosition="130" YPosition="340" OwnPage="1" ItemID="17" ItemType="6" Width="20" Height="20" FrameType="0"
                  AutoName="child" LineColor="Ink" LineWidth="1" LinePenStyle="1" Path="M0 0 L20 0 L20 20 L0 20 L0 0 Z"
                  ContourLinePath="M0 0 L20 0 L20 20 L0 20 L0 0 Z" Layer="0"/>
      <PageObject XPosition="160" YPosition="340" OwnPage="1" ItemID="18" ItemType="4" Width="20" Height="20" FrameType="0"
                  AutoName="nestedText" Columns="1" ColumnGap="0" TextToFrameDistanceLeft="0" TextToFrameDistanceTop="0"
                  TextToFrameDistanceBottom="0" TextToFrameDistanceRight="0" VerticalAlignment="0" TextPathType="0" TextPathFlipped="0"
                  Path="M0 0 L20 0 L20 20 L0 20 L0 0 Z" ContourLinePath="M0 0 L20 0 L20 20 L0 20 L0 0 Z"
                  Layer="0" NextItem="-1" BackItem="-1">
        <StoryText>
          <DefaultStyle/>
          <Content Chars="Nested"/>
          <trail/>
        </StoryText>
      </PageObject>
    </PageObject>
  </Document>
</SCRIBUSUTF8NEW>
"##;

fn import_fixture() -> Imported {
    import_scribus_report(FIXTURE.as_bytes(), &|link| (link == "pic.png").then(|| b"PNGDATA".to_vec())).expect("fixture imports")
}

#[test]
fn detects_scribus_files() {
    assert!(is_scribus(FIXTURE.as_bytes()));
    assert!(is_scribus(b"\xEF\xBB\xBF<SCRIBUSUTF8 Version=\"1.4\"></SCRIBUSUTF8>"));
    assert!(is_scribus(b"<SCRIBUSUTF8OLD></SCRIBUSUTF8OLD>"));
    assert!(is_scribus(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- made by Scribus -->\n<SCRIBUSUTF8NEW></SCRIBUSUTF8NEW>"));
    assert!(!is_scribus(b"<html></html>"));
    assert!(!is_scribus(b""));
    // Native documents quote `.sla` text without being one: the root element decides.
    assert!(!is_scribus(b"{\"stories\":{\"0\":{\"text\":\"<SCRIBUSUTF8NEW>\"}}}"));
    assert!(!is_scribus(b"<?xml version=\"1.0\"?>\n<html><p>&lt;SCRIBUSUTF8NEW</p></html>"));
}

#[test]
fn rejects_documents_that_are_not_scribus() {
    let err = import_scribus(b"<html><body/></html>").expect_err("not scribus");
    assert!(matches!(err, ScribusError::NotScribus(_)), "{err:?}");
    let err = import_scribus(b"<SCRIBUSUTF8NEW><NoDocument/></SCRIBUSUTF8NEW>").expect_err("no document");
    assert!(matches!(err, ScribusError::Invalid(_)), "{err:?}");
    assert!(import_scribus(b"<SCRIBUSUTF8NEW><Document>").is_err(), "unbalanced XML");
}

#[test]
fn deeply_nested_items_are_refused_not_overflowed() {
    let mut xml = String::from("<SCRIBUSUTF8NEW><Document PageCount=\"1\"><Page PageNumber=\"0\" PageXPosition=\"0\" PageYPosition=\"0\"/>");
    xml.push_str("<PageObject XPosition=\"0\" YPosition=\"0\" OwnPage=\"0\" ItemType=\"12\" Width=\"10\" Height=\"10\">");
    for _ in 0..300 {
        xml.push_str("<PageObject XPosition=\"0\" YPosition=\"0\" OwnPage=\"0\" ItemType=\"12\" Width=\"10\" Height=\"10\">");
    }
    xml.push_str("</PageObject>");
    assert!(import_scribus(xml.as_bytes()).is_err(), "nesting past the budget is an error");
}

#[test]
fn imports_pages_styles_guides_and_sections() {
    let im = import_fixture();
    let d = &im.document;
    assert_eq!(d.title, "Fixture");
    assert_eq!(d.file_info.author, "Tester");
    assert_eq!(d.settings.page_width, 300.0);
    assert_eq!(d.settings.page_height, 400.0);
    assert!(d.settings.facing_pages);
    assert_eq!(d.settings.bleed, [0.0; 4]);
    assert_eq!(d.settings.baseline_grid.increment, 12.0);
    assert_eq!(d.settings.baseline_grid.start, 6.0);
    assert_eq!(d.spreads.len(), 1, "both pages share one spread");
    let spread = &d.spreads[0];
    assert_eq!(spread.pages.len(), 2);
    assert_eq!(spread.pages[0].side, designcraft_doc::PageSide::Left);
    assert_eq!(spread.pages[0].x, 0.0);
    assert_eq!(spread.pages[1].side, designcraft_doc::PageSide::Right);
    assert_eq!(spread.pages[1].x, 300.0);
    assert!(spread.pages[0].parent.is_some(), "master page applied");
    // Scribus stores BorderLeft as the inside (gutter) margin for every page, left or right.
    assert_eq!(spread.pages[0].margins.inside, 10.0, "left page: BorderLeft is the gutter margin");
    assert_eq!(spread.pages[0].margins.outside, 20.0);
    assert_eq!(spread.pages[1].margins.inside, 10.0, "right page: the same gutter margin");
    assert_eq!(spread.pages[1].margins.outside, 20.0);
    let vertical = spread.pages[0].guides.iter().find(|g| g.orientation == designcraft_doc::Orientation::Vertical).expect("vertical guide");
    assert_eq!(vertical.position, 50.0);
    assert_eq!(d.sections[0].marker, "Front");
    assert_eq!(d.sections[0].start_number, Some(1));
    assert_eq!(d.parents.len(), 1);
    assert_eq!(d.parents[0].parent.as_ref().map(|p| p.name.as_str()), Some("Master A"));
    assert_eq!(d.parents[0].items.len(), 1, "master footer imported");
    assert_eq!(d.layers.len(), 2);
    assert_eq!(im.warnings.len(), 0, "{:?}", im.warnings);
}

#[test]
fn imports_items_in_spread_coordinates() {
    let d = import_fixture().document;
    let s = &d.spreads[0];
    let find = |name: &str| s.items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("missing item {name}"));
    let body = find("body");
    assert_eq!(body.xf.as_coeffs()[4], 20.0, "120 - spread origin 100");
    assert_eq!(body.xf.as_coeffs()[5], 20.0, "40 - page group top 20");
    assert!(body.is_text_frame());
    let tf = body.text_frame().expect("text frame");
    assert_eq!(tf.options.columns, 2);
    assert_eq!(tf.options.gutter, 12.0);
    assert_eq!(tf.options.inset, [6.0, 5.0, 7.0, 8.0]);
    assert_eq!(tf.options.vertical_justification, designcraft_doc::VerticalJustification::Center);

    let rotated = find("rot");
    assert_eq!(rotated.xf.as_coeffs()[4], 330.0, "430 - spread origin 100");
    assert!((rotated.xf.as_coeffs()[1].atan2(rotated.xf.as_coeffs()[0]).to_degrees() - 30.0).abs() < 1e-9);
    assert_eq!(rotated.fill.swatch, "Mist");
    assert!((rotated.fill.tint - 0.75).abs() < 1e-6);
    assert_eq!(rotated.stroke.weight, 2.5);

    let line = find("line");
    assert_eq!(line.shape, designcraft_doc::Shape::GraphicLine);
    assert_eq!(line.path.bounds().map(|b| b.width()), Some(120.0));

    let group = find("Group");
    assert!(group.is_group());
    let children = group.children();
    assert_eq!(children.len(), 2);
    assert_eq!(children[0].xf.as_coeffs()[4], 10.0, "child position is relative to the group");
    assert_eq!(children[0].xf.as_coeffs()[5], 10.0);
    let nested = children.iter().find(|c| c.name == "nestedText").expect("nested text frame");
    assert!(nested.is_text_frame());
    let nested_story = d.stories.values().find(|s| s.text == "Nested").expect("nested text got a story");
    assert_eq!(nested_story.frames, vec![nested.id]);

    let pic = find("pic");
    let graphic = pic.graphic().expect("graphic");
    assert_eq!(graphic.size, (80.0, 50.0), "no pixel size for synthetic bytes");
    assert_eq!(graphic.xf.as_coeffs()[0], 0.5);
    assert_eq!(graphic.xf.as_coeffs()[3], 0.25);
    // Scribus stores the image offset in image pixels, before the scale, and a positive
    // offset moves the picture right and down: +2 * 0.5 and -4 * 0.25 become points here.
    assert_eq!(graphic.xf.as_coeffs()[4], 1.0);
    assert_eq!(graphic.xf.as_coeffs()[5], -1.0);
}

#[test]
fn imports_threaded_stories_and_character_overrides() {
    let d = import_fixture().document;
    let stories: Vec<_> = d.stories.values().collect();
    let story = stories
        .iter()
        .find(|s| s.text.starts_with("Hello"))
        .unwrap_or_else(|| panic!("body story missing: {:?}", stories.iter().map(|s| &s.text).collect::<Vec<_>>()));
    assert_eq!(story.text, "Hello\nWorld");
    assert_eq!(story.frames.len(), 2, "the thread follows NextItem");
    assert_eq!(story.paras.len(), 2);
    assert_eq!(story.paras[0].style, "pHead", "the paragraph's trailing <para> names its style");
    assert_eq!(story.paras[1].style, "pBody");
    let big = story.chars.iter().find(|r| r.format.style == "cBig").unwrap_or_else(|| panic!("no cBig run: {:?}", story.chars));
    assert_eq!(big.format.over.size, Some(20.0), "direct size overrides the style");
    let style = d.styles.char_style_attrs("cBig");
    assert_eq!(style.font_family.as_deref(), Some("Test Font"));
    assert_eq!(style.font_style.as_deref(), Some("Bold"));
    assert_eq!(style.h_scale, Some(0.9));
    assert_eq!(style.v_scale, Some(0.8));
    assert_eq!(style.tracking, Some(120.0));
    assert_eq!(style.baseline_shift, Some(4.5), "25% of the style's 18pt");
    let (body_para, _) = d.styles.resolve_para(&story.paras[1]);
    assert_eq!(body_para.left_indent, 4.0);
    assert_eq!(body_para.first_line_indent, 6.0);
    assert_eq!(body_para.drop_cap_lines, 3);
    assert_eq!(body_para.align, designcraft_doc::Align::LeftJustified, "Scribus 3 = block: the last line stays left");
    assert_eq!(body_para.auto_leading, 1.2, "automatic line spacing is the font leading, not the stored value as a percentage");
    assert_eq!(d.styles.resolve_para_style("pForced").0.align, designcraft_doc::Align::FullyJustified, "Scribus 4 = forced");
    assert_eq!(d.styles.resolve_para(&story.paras[1]).1.leading, designcraft_doc::Leading::Points(16.0), "Scribus line spacing becomes leading");
    assert_eq!(d.styles.resolve_para(&story.paras[1]).1.language, "it", "the paragraph language drives hyphenation");
}

#[test]
fn exports_a_document_scribus_can_read_back() {
    let d = import_fixture().document;
    let exported = export_scribus(&d);
    let xml = String::from_utf8(exported.data.clone()).expect("utf-8 xml");
    assert!(xml.starts_with("<?xml version=\"1.0\""));
    assert!(xml.contains("<SCRIBUSUTF8NEW Version=\"1.7.3\">"));
    assert!(xml.contains("PageCount=\"2\""));
    assert!(xml.contains("<StoryText>"));
    assert!(xml.contains("<Layers "));
    assert_eq!(exported.images.len(), 1, "the linked image is written beside the .sla");
    assert_eq!(exported.images[0].path, "pic.png");
    assert_eq!(exported.images[0].data, b"PNGDATA");

    let back = import_scribus_report(&exported.data, &|_| None).expect("re-import");
    let d2 = &back.document;
    assert_eq!(d2.spreads.len(), d.spreads.len());
    assert_eq!(d2.stories.len(), d.stories.len());
    assert_eq!(d2.styles.paragraph.len(), d.styles.paragraph.len());
    assert_eq!(d2.styles.character.len(), d.styles.character.len());
    assert_eq!(d2.spreads[0].pages.len(), 2);
    assert_eq!(d2.spreads[0].pages[0].margins, d.spreads[0].pages[0].margins, "page margins survive the round trip");
    assert_eq!(d2.parents[0].pages[0].margins, d.parents[0].pages[0].margins, "master margins survive the round trip");
    assert_eq!(d2.spreads[0].items.len(), d.spreads[0].items.len());
    let text = d2.stories.values().find(|s| s.text.starts_with("Hello")).expect("body story");
    assert_eq!(text.text, "Hello\nWorld");
    assert_eq!(text.frames.len(), 2);
    let rot = d2.spreads[0].items.iter().find(|i| i.name == "rot").expect("rot");
    let c = rot.xf.as_coeffs();
    assert!((c[1].atan2(c[0]).to_degrees() - 30.0).abs() < 1e-6);
    assert_eq!(c[4], 330.0);
}

#[test]
fn image_folder_prefixes_sidecar_paths() {
    let d = import_fixture().document;
    let opts = ExportOptions { image_folder: Some("Fixture_images/".into()) };
    let exported = export_scribus_with(&d, &opts);
    assert_eq!(exported.images[0].path, "Fixture_images/pic.png");
    let xml = String::from_utf8(exported.data).expect("xml");
    assert!(xml.contains("ImageFileName=\"Fixture_images/pic.png\""), "{xml}");
}

#[test]
fn hostile_numbers_are_ignored_rather_than_corrupting_the_document() {
    let xml = FIXTURE
        .replacen("PageWidth=\"300\"", "PageWidth=\"NaN\"", 1)
        .replacen("PageHeight=\"400\"", "PageHeight=\"-10\"", 1)
        .replacen("Rotation=\"30\"", "Rotation=\"inf\"", 1)
        .replacen("XPosition=\"430\"", "XPosition=\"1e999\"", 1);
    let d = import_scribus(xml.as_bytes()).expect("hostile numbers still import");
    assert_eq!(d.settings.page_width, 612.0, "NaN is absent");
    assert_eq!(d.settings.page_height, 1.0, "sizes are clamped to a positive page");
    let rot = d.spreads[0].items.iter().find(|i| i.name == "rot").expect("rot");
    assert_eq!(rot.xf.as_coeffs()[1], 0.0, "inf rotation reads as 0");
    assert_eq!(rot.xf.as_coeffs()[4], -100.0, "1e999 is absent; 0 - 100 remains");
}

#[test]
fn story_and_paragraph_alignment_overrides_are_imported() {
    let xml = r##"<SCRIBUSUTF8NEW Version="1.7.4"><Document PageCount="1" PageWidth="100" PageHeight="100">
      <ParagraphStyle Name="pBody" Alignment="3" LineSpacing="16"/>
      <Page PageNumber="0" PageXPosition="0" PageYPosition="0" PageWidth="100" PageHeight="100" BorderLeft="0" BorderRight="0" BorderTop="0" BorderBottom="0"/>
      <PageObject XPosition="10" YPosition="10" OwnPage="0" ItemID="1" ItemType="4" Width="80" Height="40" FrameType="0"
        AutoName="numbers" Columns="1" ColumnGap="0" TextToFrameDistanceLeft="0" TextToFrameDistanceTop="0"
        TextToFrameDistanceBottom="0" TextToFrameDistanceRight="0" VerticalAlignment="0" TextPathType="0" TextPathFlipped="0"
        Path="M0 0 L80 0 L80 40 L0 40 L0 0 Z" ContourLinePath="M0 0 L80 0 L80 40 L0 40 L0 0 Z" Layer="0" NextItem="-1" BackItem="-1">
        <StoryText>
          <DefaultStyle Parent="pBody" Alignment="2"/>
          <Content Chars="12"/>
          <trail Parent="pBody" Alignment="2"/>
        </StoryText>
      </PageObject>
      <PageObject XPosition="10" YPosition="60" OwnPage="0" ItemID="2" ItemType="4" Width="80" Height="40" FrameType="0"
        AutoName="signature" Columns="1" ColumnGap="0" TextToFrameDistanceLeft="0" TextToFrameDistanceTop="0"
        TextToFrameDistanceBottom="0" TextToFrameDistanceRight="0" VerticalAlignment="0" TextPathType="0" TextPathFlipped="0"
        Path="M0 0 L80 0 L80 40 L0 40 L0 0 Z" ContourLinePath="M0 0 L80 0 L80 40 L0 40 L0 0 Z" Layer="0" NextItem="-1" BackItem="-1">
        <StoryText>
          <DefaultStyle Parent="pBody"/>
          <Content Chars="Name"/>
          <para Parent="pBody" Alignment="2"/>
          <Content Chars="Place"/>
          <trail Parent="pBody"/>
        </StoryText>
      </PageObject>
    </Document></SCRIBUSUTF8NEW>"##;
    let d = import_scribus(xml.as_bytes()).expect("imports");
    let story = |text: &str| d.stories.values().find(|s| s.text.starts_with(text)).unwrap_or_else(|| panic!("{text} story"));
    let numbers = story("12");
    assert_eq!(
        d.styles.resolve_para(&numbers.paras[0]).0.align,
        designcraft_doc::Align::Right,
        "the story DefaultStyle carries the alignment (page-number masters)"
    );
    let signature = story("Name");
    assert_eq!(
        d.styles.resolve_para(&signature.paras[0]).0.align,
        designcraft_doc::Align::Right,
        "a trailing <para> names the paragraph before it and its overrides"
    );
    assert_eq!(d.styles.resolve_para(&signature.paras[1]).0.align, designcraft_doc::Align::LeftJustified, "pBody is block");
}

#[test]
fn image_rotation_turns_counter_clockwise() {
    let xml = r##"<SCRIBUSUTF8NEW Version="1.7.4"><Document PageCount="1" PageWidth="100" PageHeight="100">
      <Page PageNumber="0" PageXPosition="0" PageYPosition="0" PageWidth="100" PageHeight="100" BorderLeft="0" BorderRight="0" BorderTop="0" BorderBottom="0"/>
      <PageObject XPosition="0" YPosition="0" OwnPage="0" ItemID="1" ItemType="2" Width="50" Height="50" FrameType="0"
        ImageScaleX="1" ImageScaleY="1" ImageOffsetX="0" ImageOffsetY="0" ImageRotation="90" ImageFileName="pic.png"
        Path="M0 0 L50 0 L50 50 L0 50 L0 0 Z" ContourLinePath="M0 0 L50 0 L50 50 L0 50 L0 0 Z" Layer="0"/>
    </Document></SCRIBUSUTF8NEW>"##;
    let d = import_scribus_with(xml.as_bytes(), &|_| Some(vec![0u8])).expect("imports");
    let g = d.spreads[0].items[0].graphic().expect("graphic");
    let c = g.xf.as_coeffs();
    // In the y-down page space a counter-clockwise quarter turn maps (1, 0) to (0, -1).
    assert!((c[1] + 1.0).abs() < 1e-9, "rotation turns counter-clockwise: {c:?}");
    assert!((c[2] - 1.0).abs() < 1e-9, "{c:?}");
    // The export writes the angle Scribus applies counter-clockwise, so the picture keeps
    // turning the same way through an export and a re-import.
    let exported = export_scribus(&d);
    let back = import_scribus_with(&exported.data, &|_| Some(vec![0u8])).expect("re-imports");
    let c2 = back.spreads[0].items[0].graphic().expect("graphic").xf.as_coeffs();
    assert!((c2[1] + 1.0).abs() < 1e-9, "ImageRotation survives the round trip: {c2:?}");
    assert!((c2[2] - 1.0).abs() < 1e-9, "{c2:?}");
}

/// A JPEG carrying only an APP1 `Exif` segment with the given orientation tag.
fn jpeg_with_orientation(orientation: u16) -> Vec<u8> {
    let mut tiff = Vec::new();
    tiff.extend(b"II");
    tiff.extend(42u16.to_le_bytes());
    tiff.extend(8u32.to_le_bytes());
    tiff.extend(1u16.to_le_bytes());
    tiff.extend(0x0112u16.to_le_bytes());
    tiff.extend(3u16.to_le_bytes());
    tiff.extend(1u32.to_le_bytes());
    tiff.extend(orientation.to_le_bytes());
    tiff.extend([0u8; 2]);
    tiff.extend(0u32.to_le_bytes());
    let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
    let len = (2 + 6 + tiff.len()) as u16;
    jpeg.extend(len.to_be_bytes());
    jpeg.extend(b"Exif\0\0");
    jpeg.extend(tiff);
    jpeg.extend([0xFF, 0xD9]);
    jpeg
}

#[test]
fn exif_orientation_turns_the_picture_upright() {
    let xml = r##"<SCRIBUSUTF8NEW Version="1.7.4"><Document PageCount="1" PageWidth="100" PageHeight="100">
      <Page PageNumber="0" PageXPosition="0" PageYPosition="0" PageWidth="100" PageHeight="100" BorderLeft="0" BorderRight="0" BorderTop="0" BorderBottom="0"/>
      <PageObject XPosition="0" YPosition="0" OwnPage="0" ItemID="1" ItemType="2" Width="80" Height="50" FrameType="0"
        ImageScaleX="1" ImageScaleY="1" ImageOffsetX="0" ImageOffsetY="0" ImageRotation="0" ImageFileName="pic.jpg"
        Path="M0 0 L80 0 L80 50 L0 50 L0 0 Z" ContourLinePath="M0 0 L80 0 L80 50 L0 50 L0 0 Z" Layer="0"/>
    </Document></SCRIBUSUTF8NEW>"##;
    let jpeg = jpeg_with_orientation(6);
    let d = import_scribus_with(xml.as_bytes(), &|_| Some(jpeg.clone())).expect("imports");
    let g = d.spreads[0].items[0].graphic().expect("graphic");
    // Orientation 6 is a quarter turn clockwise: the raw top-left goes to the top right, so the
    // image takes the frame's 80x50 fallback size for the 50x80 upright one.
    assert_eq!(g.xf.as_coeffs(), [0.0, 1.0, -1.0, 0.0, 50.0, 0.0]);
}

#[test]
fn layers_keep_scribus_stacking_and_ids() {
    let xml = r##"<SCRIBUSUTF8NEW Version="1.7.4"><Document PageCount="1" PageWidth="100" PageHeight="100">
      <Layers Number="0" Level="0" Name="Bottom" IsViewable="1" IsPrintable="1" IsEditable="1" LayerColor="#000000"/>
      <Layers Number="1" Level="2" Name="Top" IsViewable="1" IsPrintable="1" IsEditable="1" LayerColor="#ff0000"/>
      <Layers Number="2" Level="1" Name="Middle" IsViewable="1" IsPrintable="1" IsEditable="1" LayerColor="#00ff00"/>
      <Page PageNumber="0" PageXPosition="0" PageYPosition="0" PageWidth="100" PageHeight="100" BorderLeft="0" BorderRight="0" BorderTop="0" BorderBottom="0"/>
      <PageObject XPosition="0" YPosition="0" OwnPage="0" ItemID="1" ItemType="6" Width="50" Height="50" FrameType="0"
        AutoName="onMiddle" Path="M0 0 L50 0 L50 50 L0 50 L0 0 Z" ContourLinePath="M0 0 L50 0 L50 50 L0 50 L0 0 Z" Layer="2"/>
    </Document></SCRIBUSUTF8NEW>"##;
    let d = import_scribus(xml.as_bytes()).expect("imports");
    let names: Vec<_> = d.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Top", "Middle", "Bottom"], "front to back by Level, not by Number");
    assert_eq!(d.spreads[0].items[0].layer, d.layers[1].id, "Layer=2 is the layer whose Number is 2 (Middle)");
}

#[test]
fn exported_layers_have_distinct_levels_and_font_defaults() {
    let d = import_fixture().document;
    let exported = export_scribus(&d);
    let xml = String::from_utf8(exported.data).expect("xml");
    let root = crate::xml::parse(xml.as_bytes()).expect("parse");
    let doc_el = root.find("Document").expect("Document");
    assert!(doc_el.get("FontFace").is_some_and(|v| !v.is_empty()), "the document default font is written");
    let levels: Vec<i64> = doc_el.find_all("Layers").filter_map(|l| l.get("Level").and_then(|v| v.parse().ok())).collect();
    let mut unique = levels.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), levels.len(), "duplicate levels make Scribus lose the layers: {levels:?}");
    assert!(xml.contains("Font=\"Test Font Regular\""), "the Regular face keeps its name");
}

#[test]
fn font_names_survive_the_round_trip() {
    let xml = r##"<SCRIBUSUTF8NEW Version="1.7.4"><Document PageCount="1" PageWidth="100" PageHeight="100">
      <CharacterStyle Name="Default Character Style" Font="DejaVu Sans Book" FontSize="12"/>
      <CharacterStyle Name="cEx" Font="Geist ExtraLight" FontSize="8.5"/>
      <ParagraphStyle Name="Default Paragraph Style" DefaultStyle="1" Alignment="0" LineSpacing="14"/>
      <ParagraphStyle Name="p" Alignment="0" LineSpacing="16" CParent="cEx"/>
      <Page PageNumber="0" PageXPosition="0" PageYPosition="0" PageWidth="100" PageHeight="100" BorderLeft="0" BorderRight="0" BorderTop="0" BorderBottom="0"/>
      <PageObject XPosition="0" YPosition="0" OwnPage="0" ItemID="1" ItemType="4" Width="50" Height="50" FrameType="0"
        AutoName="t" Columns="1" ColumnGap="0" TextToFrameDistanceLeft="0" TextToFrameDistanceTop="0"
        TextToFrameDistanceBottom="0" TextToFrameDistanceRight="0" VerticalAlignment="0" TextPathType="0" TextPathFlipped="0"
        Path="M0 0 L50 0 L50 50 L0 50 L0 0 Z" ContourLinePath="M0 0 L50 0 L50 50 L0 50 L0 0 Z" Layer="0" NextItem="-1" BackItem="-1">
        <StoryText>
          <DefaultStyle Parent="p"/>
          <Content Chars="x" Font="Geist Regular"/>
          <trail Parent="p"/>
        </StoryText>
      </PageObject>
    </Document></SCRIBUSUTF8NEW>"##;
    let d = import_scribus(xml.as_bytes()).expect("imports");
    let exported = export_scribus(&d);
    let xml = String::from_utf8(exported.data).expect("xml");
    for name in ["DejaVu Sans Book", "Geist ExtraLight", "Geist Regular"] {
        assert!(xml.contains(&format!("Font=\"{name}\"")), "missing {name} in {xml}");
    }
}

#[test]
fn master_objects_name_their_master_page() {
    let xml = r##"<SCRIBUSUTF8NEW Version="1.7.4"><Document PageCount="1" PageWidth="100" PageHeight="100">
      <MasterPage PageXPosition="100" PageYPosition="20" PageWidth="100" PageHeight="100" PageNumber="0" PageName="A" LeftPage="1"/>
      <MasterPage PageXPosition="100" PageYPosition="20" PageWidth="100" PageHeight="100" PageNumber="1" PageName="B" LeftPage="0"/>
      <Page PageNumber="0" PageXPosition="100" PageYPosition="20" PageWidth="100" PageHeight="100" BorderLeft="0" BorderRight="0" BorderTop="0" BorderBottom="0"/>
      <MasterObject XPosition="110" YPosition="30" OwnPage="0" ItemID="1" ItemType="6" Width="10" Height="10" FrameType="0"
        AutoName="onA" Path="M0 0 L10 0 L10 10 L0 10 L0 0 Z" ContourLinePath="M0 0 L10 0 L10 10 L0 10 L0 0 Z" Layer="0" OnMasterPage="A"/>
      <MasterObject XPosition="110" YPosition="30" OwnPage="1" ItemID="2" ItemType="6" Width="10" Height="10" FrameType="0"
        AutoName="onB" Path="M0 0 L10 0 L10 10 L0 10 L0 0 Z" ContourLinePath="M0 0 L10 0 L10 10 L0 10 L0 0 Z" Layer="0" OnMasterPage="B"/>
    </Document></SCRIBUSUTF8NEW>"##;
    let d = import_scribus(xml.as_bytes()).expect("imports");
    let exported = export_scribus(&d);
    let xml = String::from_utf8(exported.data).expect("xml");
    let root = crate::xml::parse(xml.as_bytes()).expect("parse");
    let doc_el = root.find("Document").expect("Document");
    let numbers: std::collections::HashMap<String, String> = doc_el
        .find_all("MasterPage")
        .map(|m| (m.get("PageName").unwrap_or_default().to_string(), m.get("PageNumber").unwrap_or_default().to_string()))
        .collect();
    assert_ne!(numbers.get("A"), numbers.get("B"), "each master page has its own number: {numbers:?}");
    for obj in doc_el.find_all("MasterObject") {
        let master = obj.get("OnMasterPage").unwrap_or_default();
        assert_eq!(obj.get("OwnPage"), numbers.get(master).map(String::as_str), "object {master} names its master page");
    }
}

#[test]
fn exported_document_declares_the_spread_gap() {
    // Scribus lays spreads out from GapVertical; without it every page after the first is shifted.
    let d = import_fixture().document;
    let exported = export_scribus(&d);
    let root = crate::xml::parse(&exported.data).expect("parse");
    let doc_el = root.find("Document").expect("Document");
    assert_eq!(doc_el.get("GapVertical"), Some("40"), "the gap items are positioned with");
}

#[test]
fn export_removes_the_exif_uprighting() {
    use std::sync::Arc;
    let mut d = import_fixture().document;
    let asset_id = d.spreads[0].items.iter().find(|i| i.name == "pic").and_then(|i| i.graphic()).map(|g| g.asset).expect("pic");
    if let Some(a) = d.assets.get_mut(&asset_id) {
        let mut asset = (**a).clone();
        asset.data = Arc::new(jpeg_with_orientation(6));
        asset.pixels = Some((80, 50));
        asset.link = Some("pic.jpg".into());
        *a = Arc::new(asset);
    }
    {
        let spread = Arc::make_mut(&mut d.spreads[0]);
        for it in &mut spread.items {
            if it.name == "pic" {
                let item = Arc::make_mut(it);
                if let designcraft_doc::Content::Graphic(g) = &mut item.content {
                    // Raw pixel space with the orientation 6 uprighting the importer applies.
                    g.size = (80.0, 50.0);
                    g.xf = designcraft_geom::Affine::new([0.0, 1.0, -1.0, 0.0, 50.0, 0.0]);
                }
            }
        }
    }
    let exported = export_scribus(&d);
    let root = crate::xml::parse(&exported.data).expect("parse");
    let doc_el = root.find("Document").expect("Document");
    let obj = doc_el.find_all("PageObject").find(|o| o.get("AutoName") == Some("pic")).expect("pic");
    // Scribus applies the EXIF turn itself, so the written transform is upright again.
    assert_eq!(obj.get("ImageRotation"), Some("0"), "{:?}", obj.attrs);
    assert_eq!(obj.get("ImageScaleX"), Some("1"));
    assert_eq!(obj.get("ImageOffsetX"), Some("0"));
}

#[test]
fn character_language_survives_the_export() {
    // `pBody` is Italian; the exporter used to write `Language="en"` for every run.
    let d = import_fixture().document;
    let xml = String::from_utf8(export_scribus(&d).data).expect("xml");
    assert!(xml.contains("Language=\"it\""), "the Italian runs keep their language: {xml}");
}

#[test]
fn paragraph_alignment_and_overrides_round_trip() {
    let xml = r##"<SCRIBUSUTF8NEW Version="1.7.4"><Document PageCount="1" PageWidth="100" PageHeight="100">
      <ParagraphStyle Name="p" Alignment="0" LineSpacing="14" LeftMargin="0" RightMargin="0" FirstIndent="0" GapBefore="0" GapAfter="0"/>
      <Page PageNumber="0" PageXPosition="0" PageYPosition="0" PageWidth="100" PageHeight="100" BorderLeft="0" BorderRight="0" BorderTop="0" BorderBottom="0"/>
      <PageObject XPosition="10" YPosition="10" OwnPage="0" ItemID="1" ItemType="4" Width="80" Height="60" FrameType="0"
        AutoName="story" Columns="1" ColumnGap="0" TextToFrameDistanceLeft="0" TextToFrameDistanceTop="0"
        TextToFrameDistanceBottom="0" TextToFrameDistanceRight="0" VerticalAlignment="0" TextPathType="0" TextPathFlipped="0"
        Path="M0 0 L80 0 L80 60 L0 60 L0 0 Z" ContourLinePath="M0 0 L80 0 L80 60 L0 60 L0 0 Z" Layer="0" NextItem="-1" BackItem="-1">
        <StoryText>
          <DefaultStyle Parent="p"/>
          <Content Chars="One"/>
          <para Parent="p" Alignment="2" LeftMargin="7" RightMargin="8" FirstIndent="9"/>
          <Content Chars="Two"/>
          <trail Parent="p" Alignment="3" LeftMargin="4"/>
        </StoryText>
      </PageObject>
    </Document></SCRIBUSUTF8NEW>"##;
    let d = import_scribus(xml.as_bytes()).expect("imports");
    let exported = export_scribus(&d);
    let back = import_scribus(&exported.data).expect("re-imports");
    let story = back.stories.values().next().expect("story");
    let first = back.styles.resolve_para(&story.paras[0]).0;
    assert_eq!(first.align, designcraft_doc::Align::Right, "the first paragraph keeps its <para> alignment");
    assert_eq!(first.left_indent, 7.0, "and its indent");
    let second = back.styles.resolve_para(&story.paras[1]).0;
    assert_eq!(second.align, designcraft_doc::Align::LeftJustified, "the <trail> paragraph keeps its alignment");
    assert_eq!(second.left_indent, 4.0);
}

#[test]
fn paragraph_alignment_uses_the_five_scribus_values() {
    use designcraft_doc::Align;
    for (value, want) in [(0, Align::Left), (1, Align::Center), (2, Align::Right), (3, Align::LeftJustified), (4, Align::FullyJustified)] {
        let xml = format!(
            r##"<SCRIBUSUTF8NEW Version="1.7.4"><Document PageCount="1" PageWidth="100" PageHeight="100">
          <ParagraphStyle Name="p" Alignment="0" LineSpacing="14"/>
          <Page PageNumber="0" PageXPosition="0" PageYPosition="0" PageWidth="100" PageHeight="100" BorderLeft="0" BorderRight="0" BorderTop="0" BorderBottom="0"/>
          <PageObject XPosition="10" YPosition="10" OwnPage="0" ItemID="1" ItemType="4" Width="80" Height="40" FrameType="0"
            AutoName="story" Columns="1" ColumnGap="0" TextToFrameDistanceLeft="0" TextToFrameDistanceTop="0"
            TextToFrameDistanceBottom="0" TextToFrameDistanceRight="0" VerticalAlignment="0" TextPathType="0" TextPathFlipped="0"
            Path="M0 0 L80 0 L80 40 L0 40 L0 0 Z" ContourLinePath="M0 0 L80 0 L80 40 L0 40 L0 0 Z" Layer="0" NextItem="-1" BackItem="-1">
            <StoryText><DefaultStyle Parent="p"/><Content Chars="x"/><trail Parent="p" Alignment="{value}"/></StoryText>
          </PageObject>
        </Document></SCRIBUSUTF8NEW>"##
        );
        let d = import_scribus(xml.as_bytes()).expect("imports");
        let exported = export_scribus(&d);
        let root = crate::xml::parse(&exported.data).expect("parse");
        let trail = root
            .find("Document")
            .and_then(|doc| doc.find("PageObject"))
            .and_then(|object| object.find("StoryText"))
            .and_then(|story| story.find("trail"))
            .expect("trail");
        let written: i64 = trail.get("Alignment").and_then(|v| v.parse().ok()).expect("alignment");
        assert!((0..=4).contains(&written), "Scribus has only 0..=4; wrote {written} for {want:?}");
        let back = import_scribus(&exported.data).expect("re-imports");
        let story = back.stories.values().next().expect("story");
        assert_eq!(back.styles.resolve_para(&story.paras[0]).0.align, want, "the alignment survives the round trip");
    }
}

#[test]
fn every_exif_orientation_inverse_composes_to_identity() {
    let (w, h) = (80.0, 50.0);
    for o in 1..=8u8 {
        let m = crate::import::exif_affine(o, w, h) * crate::export::exif_inverse(o, w, h);
        let c = m.as_coeffs();
        assert!(
            (c[0] - 1.0).abs() < 1e-9
                && c[1].abs() < 1e-9
                && c[2].abs() < 1e-9
                && (c[3] - 1.0).abs() < 1e-9
                && c[4].abs() < 1e-9
                && c[5].abs() < 1e-9,
            "orientation {o} does not round-trip: {c:?}"
        );
    }
}

#[test]
fn master_page_names_are_made_unique() {
    let xml = r##"<SCRIBUSUTF8NEW Version="1.7.4"><Document PageCount="1" PageWidth="100" PageHeight="100">
      <MasterPage PageXPosition="100" PageYPosition="20" PageWidth="100" PageHeight="100" PageNumber="0" PageName="Same" LeftPage="1"/>
      <MasterPage PageXPosition="100" PageYPosition="20" PageWidth="100" PageHeight="100" PageNumber="1" PageName="Same" LeftPage="0"/>
      <Page PageNumber="0" PageXPosition="100" PageYPosition="20" PageWidth="100" PageHeight="100" BorderLeft="0" BorderRight="0" BorderTop="0" BorderBottom="0" MasterPageName="Same"/>
    </Document></SCRIBUSUTF8NEW>"##;
    let d = import_scribus(xml.as_bytes()).expect("imports");
    assert_eq!(d.parents.len(), 2);
    let exported = export_scribus(&d);
    let root = crate::xml::parse(&exported.data).expect("parse");
    let doc_el = root.find("Document").expect("Document");
    let names: Vec<String> = doc_el.find_all("MasterPage").filter_map(|m| m.get("PageName").map(str::to_string)).collect();
    assert_eq!(names.len(), 2);
    assert_ne!(names[0], names[1], "two parents called `Same` must not share a name: {names:?}");
    for page in doc_el.find_all("Page") {
        if let Some(master) = page.get("MasterPageName").filter(|m| !m.is_empty()) {
            assert!(names.iter().any(|n| n == master), "page names a missing master `{master}`: {names:?}");
        }
    }
}

#[test]
fn hostile_page_numbers_do_not_overflow_the_side() {
    let xml = r##"<SCRIBUSUTF8NEW Version="1.7.4"><Document PageCount="1" PageWidth="100" PageHeight="100" FirstPageNumber="9223372036854775807" PagePositioning="1">
      <Page PageNumber="9223372036854775807" PageXPosition="0" PageYPosition="0" PageWidth="100" PageHeight="100" BorderLeft="0" BorderRight="0" BorderTop="0" BorderBottom="0" MasterPageName="Nope"/>
    </Document></SCRIBUSUTF8NEW>"##;
    let d = import_scribus(xml.as_bytes()).expect("huge page numbers import without panicking");
    assert_eq!(d.spreads.len(), 1);
}

#[test]
fn master_objects_on_the_second_page_come_off_its_origin() {
    use std::sync::Arc;
    let mut d = import_fixture().document;
    let width = d.parents[0].pages[0].width;
    {
        let spread = Arc::make_mut(&mut d.parents[0]);
        let mut second = spread.pages[0].clone();
        second.side = designcraft_doc::PageSide::Right;
        second.x = width;
        spread.pages.push(second);
        let item = Arc::make_mut(&mut spread.items[0]);
        item.xf = designcraft_geom::Affine::translate((width + 10.0, 20.0)) * item.xf;
    }
    let exported = export_scribus(&d);
    let root = crate::xml::parse(&exported.data).expect("parse");
    let doc_el = root.find("Document").expect("Document");
    let obj = doc_el.find_all("MasterObject").next().expect("master object");
    let x: f64 = obj.get("XPosition").and_then(|v| v.parse().ok()).expect("XPosition");
    // Master pages are written one page wide at x = 100: an item on the second page must land
    // within it, not one page width further right.
    assert!(x >= 100.0 && x < 100.0 + width, "master object at {x}, page width {width}");
}
