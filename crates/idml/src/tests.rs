use std::io::Write;

use designcraft_color::{Color, Swatch, SwatchValue};
use designcraft_doc::build::NewDocument;
use designcraft_doc::{CharAttrs, Document, Fill, ParaFormat, Shape, SpreadRef, story as st};
use designcraft_geom::{Rect, shapes};

use super::*;

mod decorations;

fn zip_files(files: &[(&str, &str)]) -> Vec<u8> {
    use zip::write::SimpleFileOptions;
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    w.start_file("mimetype", stored).unwrap();
    w.write_all(MIMETYPE.as_bytes()).unwrap();
    for (n, c) in files {
        w.start_file(*n, SimpleFileOptions::default()).unwrap();
        w.write_all(c.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn zip_files_plain() -> Vec<u8> {
    use zip::write::SimpleFileOptions;
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    w.start_file("document.json", SimpleFileOptions::default()).unwrap();
    w.write_all(b"{}").unwrap();
    w.finish().unwrap().into_inner()
}

/// The text of every part of the package `bytes` whose name starts with `prefix`.
fn zip_text(bytes: &[u8], prefix: &str) -> String {
    use std::io::Read;
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut out = String::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        if f.name().starts_with(prefix) {
            f.read_to_string(&mut out).unwrap();
        }
    }
    out
}

/// A hand-written, minimal IDML document: one facing-pages spread with a right page, a
/// threaded story across two frames, a tint and a grouped paragraph style.
const DESIGNMAP: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<?aid style="50" type="document" readerVersion="6.0" featureSet="257" product="16.0(1)" ?>
<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0" Self="d" StoryList="s1" Name="Fixture.indd">
  <idPkg:Graphic src="Resources/Graphic.xml"/>
  <idPkg:Styles src="Resources/Styles.xml"/>
  <idPkg:Preferences src="Resources/Preferences.xml"/>
  <Layer Self="L1" Name="Art" Visible="true" Locked="false"><Properties><LayerColor type="enumeration">Red</LayerColor></Properties></Layer>
  <idPkg:MasterSpread src="MasterSpreads/MasterSpread_m1.xml"/>
  <idPkg:Spread src="Spreads/Spread_sp1.xml"/>
  <Section Self="sec" Length="2" ContinueNumbering="false" PageNumberStart="5" PageStart="p1" SectionPrefix="" Marker="">
    <Properties><PageNumberStyle type="enumeration">LowerRoman</PageNumberStyle></Properties>
  </Section>
  <idPkg:Story src="Stories/Story_s1.xml"/>
</Document>"#;

const GRAPHIC: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<idPkg:Graphic xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <Color Self="Color/Black" Model="Process" Space="CMYK" ColorValue="0 0 0 100" Name="Black"/>
  <Color Self="Color/Brand" Model="Spot" Space="RGB" ColorValue="255 0 0" Name="Brand" Visible="true"/>
  <Color Self="Color/u9" Model="Process" Space="CMYK" ColorValue="10 20 30 40" Name="$ID/" Visible="false"/>
  <Tint Self="Tint/Brand 50%25" BaseColor="Color/Brand" TintValue="50" Name="Brand 50%"/>
  <Swatch Self="Swatch/None" Name="None"/>
  <Gradient Self="Gradient/Fade" Type="Radial" Name="Fade">
    <GradientStop Self="g0" StopColor="Color/Brand" Location="0"/>
    <GradientStop Self="g1" StopColor="Color/u9" Location="100" Midpoint="30"/>
  </Gradient>
</idPkg:Graphic>"#;

const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<idPkg:Styles xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <RootCharacterStyleGroup Self="rc">
    <CharacterStyle Self="CharacterStyle/$ID/[No character style]" Name="$ID/[No character style]"/>
    <CharacterStyle Self="CharacterStyle/Strong" Name="Strong" FontStyle="Bold">
      <Properties><BasedOn type="string">$ID/[No character style]</BasedOn></Properties>
    </CharacterStyle>
  </RootCharacterStyleGroup>
  <RootParagraphStyleGroup Self="rp">
    <ParagraphStyle Self="ParagraphStyle/$ID/[No paragraph style]" Name="$ID/[No paragraph style]" PointSize="12">
      <Properties><AppliedFont type="string">Source Serif 4</AppliedFont><Leading type="enumeration">Auto</Leading></Properties>
    </ParagraphStyle>
    <ParagraphStyle Self="ParagraphStyle/$ID/NormalParagraphStyle" Name="$ID/NormalParagraphStyle">
      <Properties><BasedOn type="string">$ID/[No paragraph style]</BasedOn></Properties>
    </ParagraphStyle>
    <ParagraphStyleGroup Self="ParagraphStyleGroup/Text" Name="Text">
      <ParagraphStyle Self="ParagraphStyle/Text%3aBody" Name="Text:Body" PointSize="10" Justification="LeftJustified" SpaceAfter="4">
        <Properties><BasedOn type="object">ParagraphStyle/$ID/NormalParagraphStyle</BasedOn><Leading type="unit">13</Leading></Properties>
      </ParagraphStyle>
    </ParagraphStyleGroup>
  </RootParagraphStyleGroup>
  <RootObjectStyleGroup Self="ro">
    <ObjectStyle Self="ObjectStyle/$ID/[None]" Name="$ID/[None]"/>
    <ObjectStyle Self="ObjectStyle/$ID/[Normal Graphics Frame]" Name="$ID/[Normal Graphics Frame]" EnableStroke="true" StrokeColor="Color/Black" StrokeWeight="1"/>
  </RootObjectStyleGroup>
</idPkg:Styles>"#;

const PREFS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<idPkg:Preferences xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <DocumentPreference PageWidth="500" PageHeight="700" FacingPages="true" DocumentBleedTopOffset="9"/>
  <MarginPreference ColumnCount="2" ColumnGutter="10" Top="20" Bottom="30" Left="40" Right="50"/>
</idPkg:Preferences>"#;

const MASTER: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<idPkg:MasterSpread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <MasterSpread Self="m1" Name="A-Parent" NamePrefix="A" BaseName="Parent" PageCount="2">
    <Page Self="mp1" GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 -500 -350"/>
    <Page Self="mp2" GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 0 -350"/>
    <Rectangle Self="mr" ItemLayer="L1" ItemTransform="1 0 0 1 0 0" FillColor="Color/u9">
      <Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
        <PathPointType Anchor="-480 -330" LeftDirection="-480 -330" RightDirection="-480 -330"/>
        <PathPointType Anchor="-480 -300" LeftDirection="-480 -300" RightDirection="-480 -300"/>
        <PathPointType Anchor="-400 -300" LeftDirection="-400 -300" RightDirection="-400 -300"/>
        <PathPointType Anchor="-400 -330" LeftDirection="-400 -330" RightDirection="-400 -330"/>
      </PathPointArray></GeometryPathType></PathGeometry></Properties>
    </Rectangle>
  </MasterSpread>
</idPkg:MasterSpread>"#;

const SPREAD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <Spread Self="sp1" PageCount="2" ItemTransform="1 0 0 1 0 0">
    <Page Self="p1" AppliedMaster="m1" GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 -500 -350">
      <MarginPreference ColumnCount="3" ColumnGutter="12" Top="10" Bottom="10" Left="20" Right="30"/>
    </Page>
    <Page Self="p2" AppliedMaster="m1" GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 0 -350"/>
    <TextFrame Self="t1" ParentStory="s1" PreviousTextFrame="n" NextTextFrame="t2" ItemLayer="L1" ItemTransform="1 0 0 1 -450 -300" FillColor="Tint/Brand 50%25">
      <Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
        <PathPointType Anchor="0 0" LeftDirection="0 0" RightDirection="0 0"/>
        <PathPointType Anchor="0 200" LeftDirection="0 200" RightDirection="0 200"/>
        <PathPointType Anchor="300 200" LeftDirection="300 200" RightDirection="300 200"/>
        <PathPointType Anchor="300 0" LeftDirection="300 0" RightDirection="300 0"/>
      </PathPointArray></GeometryPathType></PathGeometry></Properties>
      <TextFramePreference TextColumnCount="2" TextColumnGutter="8"><Properties><InsetSpacing type="list"><ListItem type="unit">1</ListItem><ListItem type="unit">2</ListItem><ListItem type="unit">3</ListItem><ListItem type="unit">4</ListItem></InsetSpacing></Properties></TextFramePreference>
      <UnknownThing Foo="bar"><Nested/></UnknownThing>
    </TextFrame>
    <TextFrame Self="t2" ParentStory="s1" PreviousTextFrame="t1" NextTextFrame="n" ItemLayer="L1" ItemTransform="1 0 0 1 50 -300">
      <Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
        <PathPointType Anchor="0 0"/><PathPointType Anchor="0 100"/><PathPointType Anchor="100 100"/><PathPointType Anchor="100 0"/>
      </PathPointArray></GeometryPathType></PathGeometry></Properties>
    </TextFrame>
    <Group Self="g" ItemTransform="1 0 0 1 10 20">
      <Oval Self="o" ItemTransform="1 0 0 1 0 0" FillColor="Gradient/Fade" StrokeWeight="2" StrokeColor="Color/Brand">
        <Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
          <PathPointType Anchor="0 0"/><PathPointType Anchor="0 50"/><PathPointType Anchor="50 50"/><PathPointType Anchor="50 0"/>
        </PathPointArray></GeometryPathType></PathGeometry></Properties>
      </Oval>
    </Group>
    <Rectangle Self="img" ItemLayer="L1" ItemTransform="1 0 0 1 100 100" ContentType="GraphicType" AppliedObjectStyle="ObjectStyle/$ID/[Normal Graphics Frame]">
      <Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
        <PathPointType Anchor="0 0"/><PathPointType Anchor="0 30"/><PathPointType Anchor="40 30"/><PathPointType Anchor="40 0"/>
      </PathPointArray></GeometryPathType></PathGeometry></Properties>
      <Image Self="im" ItemTransform="1 0 0 1 0 0">
        <Properties><GraphicBounds Left="0" Top="0" Right="40" Bottom="30"/></Properties>
        <Link Self="lk" LinkResourceURI="file:/definitely/missing%20dir/photo.jpg" LinkResourceFormat="$ID/JPEG" StoredState="Normal"/>
      </Image>
    </Rectangle>
  </Spread>
</idPkg:Spread>"#;

const STORY: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <Story Self="s1">
    <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/Text%3aBody">
      <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]">
        <Content>Hello &amp; </Content>
      </CharacterStyleRange>
      <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/Strong" PointSize="14">
        <Content>bold</Content>
        <Br/>
      </CharacterStyleRange>
    </ParagraphStyleRange>
    <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/NormalParagraphStyle" Justification="CenterAlign">
      <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]">
        <Content>Page <?ACE 18?>	end</Content>
      </CharacterStyleRange>
      <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" ParagraphBreakType="NextFrame"><Br/></CharacterStyleRange>
      <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>after</Content></CharacterStyleRange>
    </ParagraphStyleRange>
  </Story>
</idPkg:Story>"#;

fn fixture() -> Vec<u8> {
    zip_files(&[
        ("designmap.xml", DESIGNMAP),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Styles.xml", STYLES),
        ("Resources/Preferences.xml", PREFS),
        ("MasterSpreads/MasterSpread_m1.xml", MASTER),
        ("Spreads/Spread_sp1.xml", SPREAD),
        ("Stories/Story_s1.xml", STORY),
    ])
}

fn fixture_with_story(story: &str) -> Vec<u8> {
    zip_files(&[
        ("designmap.xml", DESIGNMAP),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Styles.xml", STYLES),
        ("Resources/Preferences.xml", PREFS),
        ("MasterSpreads/MasterSpread_m1.xml", MASTER),
        ("Spreads/Spread_sp1.xml", SPREAD),
        ("Stories/Story_s1.xml", story),
    ])
}

/// InDesign: a break character ends its paragraph (`<Br/>` with a ParagraphBreakType closes the
/// range); the text after it is a paragraph with its own style, set on the next page of the
/// break's parity.
const ODD_PAGE_BREAK_STORY: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <Story Self="s1">
    <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/Text%3aBody">
      <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>Body</Content></CharacterStyleRange>
      <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" ParagraphBreakType="NextOddPage"><Br/></CharacterStyleRange>
    </ParagraphStyleRange>
    <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/NormalParagraphStyle" Justification="CenterAlign">
      <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>1</Content></CharacterStyleRange>
    </ParagraphStyleRange>
  </Story>
</idPkg:Story>"#;

#[test]
fn odd_page_break_ends_its_paragraph_and_keeps_its_parity() {
    let mut d = import_idml_with(&fixture_with_story(ODD_PAGE_BREAK_STORY), &|_| None).unwrap();
    let sid = *d.stories.keys().next().unwrap();
    let story = &d.stories[&sid];
    assert_eq!(story.text, format!("Body{}\n1", st::ODD_PAGE_BREAK));
    assert_eq!(story.paras.len(), 2);
    assert_eq!(story.paras[0].style, "Text/Body");
    assert_eq!(story.paras[1].style, st::BASIC_PARAGRAPH);
    assert_eq!(story.paras[1].para.align, Some(designcraft_doc::Align::Center));
    // Export writes the break as the paragraph's one `Br`; importing that gives the same story.
    let back = import_idml(&export_idml(&d)).unwrap();
    let again = back.stories.values().next().unwrap();
    assert_eq!(again.text, story.text);
    assert_eq!(again.paras, story.paras);
    // The fixture's frames are on pages v and vi: the next odd page after v has no frame.
    let cs = designcraft_compose::compose_story(&d, sid, &Default::default());
    assert_eq!(cs.frames[0].lines.len(), 1, "no empty line after the break");
    assert!(cs.frames[1].lines.is_empty());
    assert_eq!(cs.overset_at, Some(story.text.len() - 1));
    // Numbered from 4, the second frame is on page 5: "1" starts there.
    d.sections[0].start_number = Some(4);
    let cs = designcraft_compose::compose_story(&d, sid, &Default::default());
    assert_eq!(cs.overset_at, None);
    assert_eq!(cs.frames[1].lines.iter().map(|l| l.para).collect::<Vec<_>>(), vec![1]);
}

#[test]
fn imports_hand_written_fixture() {
    let d = import_idml_with(&fixture(), &|_| None).unwrap();
    assert_eq!(d.title, "Fixture");
    assert!(d.settings.facing_pages);
    assert_eq!((d.settings.page_width, d.settings.page_height), (500.0, 700.0));
    assert_eq!(d.settings.bleed[0], 9.0);
    assert_eq!(d.page_count(), 2);
    let sp = &d.spreads[0];
    assert_eq!(sp.pages[0].side, designcraft_doc::PageSide::Left);
    assert_eq!(sp.pages[1].side, designcraft_doc::PageSide::Right);
    assert_eq!(sp.pages[1].x, 500.0);
    assert_eq!(sp.pages[0].margins.inside, 20.0);
    assert_eq!(sp.pages[0].columns.count, 3);
    assert_eq!(sp.pages[1].columns.count, 2, "falls back to the default margin preference");
    assert_eq!(sp.pages[0].parent, Some(d.parents[0].id));
    let info = d.parents[0].parent.as_ref().unwrap();
    assert_eq!((info.prefix.as_str(), info.name.as_str()), ("A", "Parent"));
    // Sections.
    assert_eq!(d.page_name(0), "v");
    assert_eq!(d.page_name(1), "vi");
    // Layers.
    assert_eq!(d.layers[0].name, "Art");
    assert_eq!(d.layers[0].color, [255, 0, 0]);
    // Swatches.
    assert!(matches!(d.swatch("Brand").unwrap().value, SwatchValue::Color { color_type: designcraft_color::ColorType::Spot, .. }));
    assert!(matches!(&d.swatch("Brand 50%").unwrap().value, SwatchValue::Tint { base, tint } if base == "Brand" && *tint == 0.5));
    let SwatchValue::Gradient { gradient } = &d.swatch("Fade").unwrap().value else { panic!("gradient") };
    assert_eq!(gradient.kind, designcraft_color::GradientKind::Radial);
    assert_eq!(gradient.stops[1].color, Color::cmyk(0.1, 0.2, 0.3, 0.4));
    assert!((gradient.stops[0].midpoint - 0.3).abs() < 1e-6);
    // The parent rectangle used an unnamed colour → value-named swatch.
    let mr = &d.parents[0].items[0];
    assert_eq!(mr.fill.swatch, "C=10 M=20 Y=30 K=40");
    assert_eq!(mr.bounds(), Rect::new(20.0, 20.0, 100.0, 50.0));
    // Styles.
    let body = d.styles.para("Text/Body").expect("grouped style");
    assert_eq!(body.based_on.as_deref(), Some(st::BASIC_PARAGRAPH));
    assert_eq!(body.chars.size, Some(10.0));
    assert_eq!(body.chars.leading, Some(designcraft_doc::Leading::Points(13.0)));
    assert_eq!(body.para.align, Some(designcraft_doc::Align::LeftJustified));
    assert_eq!(d.styles.char_style("Strong").unwrap().chars.font_style.as_deref(), Some("Bold"));
    // Frames, threads, text.
    let story = d.stories.values().next().unwrap();
    assert_eq!(story.text, format!("Hello & bold\nPage {}\tend{}\nafter", st::PAGE_NUMBER, st::FRAME_BREAK));
    assert_eq!(story.paras.len(), 3);
    assert_eq!(story.paras[0].style, "Text/Body");
    assert_eq!(story.paras[1].style, st::BASIC_PARAGRAPH);
    assert_eq!(story.paras[1].para.align, Some(designcraft_doc::Align::Center));
    let bold = story.runs().find(|(r, _)| story.text[r.clone()].starts_with("bold")).unwrap().1;
    assert_eq!(bold.style, "Strong");
    assert_eq!(bold.over.size, Some(14.0));
    assert_eq!(story.frames.len(), 2);
    let f1 = d.item(story.frames[0]).unwrap();
    assert_eq!(f1.bounds(), Rect::new(50.0, 50.0, 350.0, 250.0));
    assert_eq!(f1.fill.swatch, "Brand 50%");
    let o = &f1.text_frame().unwrap().options;
    assert_eq!((o.columns, o.gutter, o.inset), (2, 8.0, [1.0, 2.0, 3.0, 4.0]));
    assert_eq!(d.item(story.frames[1]).unwrap().bounds(), Rect::new(550.0, 50.0, 650.0, 150.0));
    // Group child keeps its own transform relative to the group.
    let g = sp.items.iter().find(|i| i.shape == Shape::Group).unwrap();
    assert_eq!(g.bounds(), Rect::new(510.0, 370.0, 560.0, 420.0));
    let oval = &g.children()[0];
    assert_eq!(oval.fill.swatch, "Fade");
    assert_eq!((oval.stroke.swatch.as_str(), oval.stroke.weight), ("Brand", 2.0));
    // Linked image with a missing file: link recorded, no data; stroke from the object style.
    let img = sp.items.iter().find(|i| i.graphic().is_some()).unwrap();
    assert_eq!(img.stroke.swatch, "[Black]");
    let a = &d.assets[&img.graphic().unwrap().asset];
    assert_eq!(a.link.as_deref(), Some("/definitely/missing dir/photo.jpg"));
    assert!(a.data.is_empty());
    assert_eq!(a.mime, "image/jpeg");
    assert_eq!(a.name, "photo.jpg");
}

/// Hand-written inset encodings, independent of the exporter's four-item list.
fn inset_fixture(preference: &str, object_styles: &str) -> Document {
    let designmap = format!(
        r#"<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" Self="d">
          <RootObjectStyleGroup Self="ro">{object_styles}</RootObjectStyleGroup>
          <idPkg:Spread src="Spreads/Spread_s.xml"/>
        </Document>"#
    );
    let spread = format!(
        r#"<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
          <Spread Self="s">
            <Page Self="p" GeometricBounds="0 0 100 100" ItemTransform="1 0 0 1 0 0"/>
            <TextFrame Self="f" AppliedObjectStyle="ObjectStyle/Padded">
              <Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
                <PathPointType Anchor="0 0"/><PathPointType Anchor="0 100"/>
                <PathPointType Anchor="100 100"/><PathPointType Anchor="100 0"/>
              </PathPointArray></GeometryPathType></PathGeometry></Properties>
              {preference}
            </TextFrame>
          </Spread>
        </idPkg:Spread>"#
    );
    import_idml(&zip_files(&[("designmap.xml", &designmap), ("Spreads/Spread_s.xml", &spread)])).unwrap()
}

fn frame_inset(d: &Document) -> [f64; 4] {
    d.spreads[0].items[0].text_frame().unwrap().options.inset
}

fn inset_list(values: &[&str]) -> String {
    let items = values.iter().map(|v| format!(r#"<ListItem type="unit">{v}</ListItem>"#)).collect::<String>();
    format!(r#"<Properties><InsetSpacing type="list">{items}</InsetSpacing></Properties>"#)
}

#[test]
fn imports_scalar_inset_properties_like_lists_and_attributes() {
    let scalar =
        inset_fixture(r#"<TextFramePreference><Properties><InsetSpacing type="unit"> 9 </InsetSpacing></Properties></TextFramePreference>"#, "");
    let list = inset_fixture(&format!("<TextFramePreference>{}</TextFramePreference>", inset_list(&["9"; 4])), "");
    let attribute = inset_fixture(r#"<TextFramePreference InsetSpacing="9"/>"#, "");
    let untyped = inset_fixture(r#"<TextFramePreference><Properties><InsetSpacing>9</InsetSpacing></Properties></TextFramePreference>"#, "");
    assert_eq!(frame_inset(&scalar), [9.0; 4]);
    assert_eq!(frame_inset(&scalar), frame_inset(&list));
    assert_eq!(frame_inset(&scalar), frame_inset(&attribute));
    assert_eq!(frame_inset(&scalar), frame_inset(&untyped));
    assert_eq!(scalar.spreads[0].items[0].text_area(), Rect::new(9.0, 9.0, 91.0, 91.0));
    let round_trip = import_idml(&export_idml(&scalar)).unwrap();
    assert_eq!(frame_inset(&round_trip), [9.0; 4]);
}

#[test]
fn inset_property_keeps_precedence_over_attribute() {
    for (property, expected) in [
        (r#"<Properties><InsetSpacing type="unit">9</InsetSpacing></Properties>"#.to_string(), [9.0; 4]),
        (inset_list(&["1", "2", "3", "4"]), [1.0, 2.0, 3.0, 4.0]),
        (inset_list(&["1", "2", "3"]), [0.0; 4]),
        (r#"<Properties><InsetSpacing type="unit">invalid</InsetSpacing></Properties>"#.to_string(), [0.0; 4]),
    ] {
        let d = inset_fixture(&format!(r#"<TextFramePreference InsetSpacing="42">{property}</TextFramePreference>"#), "");
        assert_eq!(frame_inset(&d), expected, "{property}");
    }
}

#[test]
fn inset_spacing_rejects_malformed_and_nonfinite_values() {
    let mut preferences = vec![
        String::new(),
        "<TextFramePreference/>".to_string(),
        r#"<TextFramePreference><Properties><InsetSpacing type="list">9</InsetSpacing></Properties></TextFramePreference>"#.to_string(),
        r#"<TextFramePreference><Properties><InsetSpacing type="unit"><Other>9</Other></InsetSpacing></Properties></TextFramePreference>"#
            .to_string(),
    ];
    for value in ["", "invalid", "NaN", "inf", "-inf", "1e309"] {
        preferences.push(format!(r#"<TextFramePreference InsetSpacing="{value}"/>"#));
        preferences
            .push(format!(r#"<TextFramePreference><Properties><InsetSpacing type="unit">{value}</InsetSpacing></Properties></TextFramePreference>"#));
        preferences.push(format!("<TextFramePreference>{}</TextFramePreference>", inset_list(&["1", value, "3", "4"])));
    }
    for values in [&[][..], &["9"][..], &["1", "2", "3"][..], &["1", "2", "3", "4", "5"][..], &["1", "bad", "2", "3", "4"][..]] {
        preferences.push(format!("<TextFramePreference>{}</TextFramePreference>", inset_list(values)));
    }
    preferences.push(format!("<TextFramePreference>{}</TextFramePreference>", inset_list(&["1", "<Other>2</Other>", "3", "4"])));
    for preference in preferences {
        assert_eq!(frame_inset(&inset_fixture(&preference, "")), [0.0; 4], "{preference}");
    }
}

#[test]
fn inset_spacing_preserves_finite_signed_values_and_edge_order() {
    for value in ["0", "-2.5", " 4.5 "] {
        let expected = [value.trim().parse::<f64>().unwrap(); 4];
        for preference in [
            format!(r#"<TextFramePreference InsetSpacing="{value}"/>"#),
            format!(r#"<TextFramePreference><Properties><InsetSpacing type="unit">{value}</InsetSpacing></Properties></TextFramePreference>"#),
            format!("<TextFramePreference>{}</TextFramePreference>", inset_list(&[value; 4])),
        ] {
            assert_eq!(frame_inset(&inset_fixture(&preference, "")), expected, "{preference}");
        }
    }
    let d = inset_fixture(&format!("<TextFramePreference>{}</TextFramePreference>", inset_list(&["1", "-2", "3.5", "4"])), "");
    assert_eq!(frame_inset(&d), [1.0, -2.0, 3.5, 4.0], "top, left, bottom, right");
}

#[test]
fn imports_scalar_insets_in_object_style_without_replacing_frame_override() {
    let styles = r#"<ObjectStyle Self="ObjectStyle/Padded" Name="Padded" EnableTextFrameGeneralOptions="true">
      <TextFramePreference><Properties><InsetSpacing type="unit">9</InsetSpacing></Properties></TextFramePreference>
    </ObjectStyle>"#;
    let d = inset_fixture(r#"<TextFramePreference InsetSpacing="3"/>"#, styles);
    assert_eq!(d.styles.object.iter().find(|s| s.name == "Padded").unwrap().text_frame.as_ref().unwrap().inset, [9.0; 4]);
    assert_eq!(frame_inset(&d), [3.0; 4]);
}

#[test]
fn rejects_non_idml() {
    assert!(import_idml(b"not a zip").is_err());
    let z = zip_files(&[("hello.txt", "x")]);
    assert!(matches!(import_idml(&z), Err(IdmlError::NotIdml(_))));
}

fn small_doc() -> Document {
    let mut d = Document::new(&NewDocument { pages: 3, ..Default::default() });
    d.swatches.push(Swatch::color("Brand", Color::rgb(1.0, 0.0, 0.0)));
    d.swatches.push(Swatch {
        name: "Brand 30".into(),
        value: SwatchValue::Tint { base: "Brand".into(), tint: 0.3 },
        locked: false,
        named: true,
        hidden: false,
    });
    let lid = d.default_layer();
    let (f1, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(40.0, 40.0, 300.0, 200.0), lid, "One\ttwo\nThree", ParaFormat::default()).unwrap();
    let (f2, _) = d.add_text_frame(SpreadRef::Doc(1), Rect::new(700.0, 40.0, 900.0, 200.0), lid, "", ParaFormat::default()).unwrap();
    d.thread(f1, f2).unwrap();
    if let Some(s) = d.story_mut(sid) {
        s.format_chars(0..3, |f| f.over = CharAttrs { size: Some(20.0), fill: Some("Brand".into()), ..Default::default() });
        let end = s.len();
        s.insert(end, &format!(" {}\n", st::COLUMN_BREAK));
    }
    let id = designcraft_doc::ItemId(d.alloc());
    let mut it = designcraft_doc::Item::new(id, lid, Shape::Oval, shapes::ellipse(Rect::new(650.0, 300.0, 750.0, 380.0)));
    it.fill = Fill::swatch("Brand 30");
    it.xf = designcraft_geom::Affine::rotate_about(0.3, designcraft_geom::Point::new(700.0, 340.0));
    it.opacity = 0.5;
    d.insert_item(SpreadRef::Doc(1), it, None).unwrap();
    d
}

#[test]
fn round_trips_small_document() {
    let d = small_doc();
    let bytes = export_idml(&d);
    // The mimetype is the first, stored entry.
    assert_eq!(&bytes[30..38], b"mimetype");
    assert_eq!(&bytes[38..38 + MIMETYPE.len()], MIMETYPE.as_bytes());
    assert!(is_idml(&bytes));
    assert!(!is_idml(&zip_files_plain()));
    let back = import_idml(&bytes).unwrap();
    assert_eq!(back.page_count(), d.page_count());
    assert_eq!(back.spreads.len(), d.spreads.len());
    for (a, b) in d.spreads.iter().zip(&back.spreads) {
        for (pa, pb) in a.pages.iter().zip(&b.pages) {
            assert!((pa.x - pb.x).abs() < 0.01 && pa.side == pb.side);
        }
        assert_eq!(a.items.len(), b.items.len());
        for (ia, ib) in a.items.iter().zip(&b.items) {
            let (ra, rb) = (ia.bounds(), ib.bounds());
            assert!((ra.x0 - rb.x0).abs() < 0.01 && (ra.y1 - rb.y1).abs() < 0.01, "{ra:?} vs {rb:?}");
        }
    }
    let sa: Vec<&str> = d.stories.values().map(|s| s.text.as_str()).collect();
    let sb: Vec<&str> = back.stories.values().map(|s| s.text.as_str()).collect();
    assert_eq!(sa, sb);
    let s = back.stories.values().next().unwrap();
    assert_eq!(s.frames.len(), 2);
    assert_eq!(s.runs().next().unwrap().1.over.size, Some(20.0));
    assert_eq!(s.runs().next().unwrap().1.over.fill.as_deref(), Some("Brand"));
    let oval = back.spreads[1].items.iter().find(|i| i.shape == Shape::Oval).unwrap();
    assert_eq!(oval.fill.swatch, "Brand 30");
    assert!((oval.opacity - 0.5).abs() < 1e-6);
    assert!(back.swatch("Brand 30").is_some());
}

#[test]
fn uri_and_base64_helpers() {
    assert_eq!(import::uri_to_path("file:/a%20b/c.png"), "/a b/c.png");
    assert_eq!(import::uri_to_path("file:///C:/x/y.jpg"), "C:/x/y.jpg");
    assert_eq!(export::path_to_uri("/a b/c.png"), "file:/a%20b/c.png");
    let data: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
    assert_eq!(base64_decode(&base64_encode(&data)), data);
}

#[test]
fn round_trips_tables() {
    let mut d = Document::new(&NewDocument::default());
    d.swatches.push(Swatch::color("Brand", Color::rgb(1.0, 0.0, 0.0)));
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(40.0, 40.0, 400.0, 400.0), lid, "Intro\nOutro", ParaFormat::default()).unwrap();
    let mut t = designcraft_doc::Table::new(d.alloc(), 3, 3, 1, 0, 300.0);
    t.cell_mut(0, 0).unwrap().text.insert(0, "Head");
    t.cell_mut(0, 0).unwrap().fill = "Brand".into();
    t.cell_mut(1, 2).unwrap().text.insert(0, "Body\nTwo");
    t.cell_mut(2, 1).unwrap().vj = designcraft_doc::VerticalJustification::Center;
    t.rows[2].mode = designcraft_doc::RowHeightMode::Exactly;
    t.rows[2].height = 30.0;
    t.columns[0].width = 50.0;
    t.merge(designcraft_doc::CellRange::new(2, 1, 3, 2)).unwrap();
    t.options.alt_rows = Some(designcraft_doc::AltFills { first: 1, first_color: "Brand".into(), first_tint: 0.2, ..Default::default() });
    d.story_mut(sid).unwrap().insert_table(5, t.clone());
    d.check().unwrap();
    let back = import_idml(&export_idml(&d)).unwrap();
    back.check().unwrap();
    let s = back.stories.values().next().unwrap();
    assert_eq!(s.text, format!("Intro\n{}\nOutro", st::TABLE_ANCHOR));
    let bt = s.tables.values().next().unwrap();
    assert_eq!((bt.nrows(), bt.ncols(), bt.header_rows()), (4, 3, 1));
    assert_eq!(bt.cell(0, 0).unwrap().text.text, "Head");
    assert_eq!(bt.cell(0, 0).unwrap().fill, "Brand");
    assert_eq!(bt.cell(1, 2).unwrap().text.text, "Body\nTwo");
    assert_eq!(bt.cell(2, 1).unwrap().vj, designcraft_doc::VerticalJustification::Center);
    assert_eq!((bt.cell(2, 1).unwrap().row_span, bt.cell(2, 1).unwrap().col_span), (2, 2));
    assert_eq!(bt.rows[2].mode, designcraft_doc::RowHeightMode::Exactly);
    assert!((bt.rows[2].height - 30.0).abs() < 1e-6);
    assert!((bt.columns[0].width - 50.0).abs() < 1e-6);
    assert_eq!(bt.options.alt_rows.as_ref().map(|a| a.first_color.as_str()), Some("Brand"));
}

#[test]
fn imports_and_round_trips_explicit_cell_border_overrides() {
    // Hand-written IDML: the outer cell edge may explicitly suppress or replace the
    // table border. An absent/zero priority keeps the table border's precedence.
    let story = r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
<Story Self="s1"><ParagraphStyleRange><CharacterStyleRange>
<Table HeaderRowCount="0" FooterRowCount="0" TopBorderStrokeWeight="2">
  <Row Name="0" MinimumHeight="24"/>
  <Column Name="0" SingleColumnWidth="60"/><Column Name="1" SingleColumnWidth="60"/>
  <Cell Name="0:0" TopEdgeStrokeColor="Swatch/None" TopEdgeStrokePriority="1"
        LeftEdgeStrokeColor="Color/Brand" LeftEdgeStrokeWeight="3" LeftEdgeStrokePriority="2"
        BottomEdgeStrokeWeight="0" BottomEdgeStrokePriority="0" RightEdgeStrokeColor="Swatch/None">
    <ParagraphStyleRange><CharacterStyleRange><Content>A</Content></CharacterStyleRange></ParagraphStyleRange>
  </Cell>
  <Cell Name="1:0"><ParagraphStyleRange><CharacterStyleRange><Content>B</Content></CharacterStyleRange></ParagraphStyleRange></Cell>
</Table></CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>"#;
    let d = import_idml(&fixture_with_story(story)).unwrap();
    d.check().unwrap();
    let table = d.stories.values().flat_map(|s| s.tables.values()).next().unwrap();
    let cell = table.cell(0, 0).unwrap();
    assert_eq!(cell.border_overrides, [true, true, false, false]);
    assert!(!cell.strokes[0].is_visible(), "explicit None is retained");
    assert_eq!(cell.strokes[1].color, "Brand");
    assert_eq!(cell.strokes[1].weight, 3.0);
    assert_eq!(table.options.border.weight, 2.0);
    assert_eq!(table.cell(0, 1).unwrap().border_overrides, [false; 4]);

    let back = import_idml(&export_idml(&d)).unwrap();
    let roundtrip = back.stories.values().flat_map(|s| s.tables.values()).next().unwrap();
    assert_eq!(roundtrip.cell(0, 0).unwrap().border_overrides, cell.border_overrides);
    assert_eq!(roundtrip.cell(0, 0).unwrap().strokes, cell.strokes);
    assert_eq!(roundtrip.cell(0, 1).unwrap().border_overrides, [false; 4]);
}

#[test]
fn round_trips_footnotes_and_options() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(40.0, 40.0, 400.0, 400.0), lid, "Body text here.", ParaFormat::default()).unwrap();
    {
        let s = d.story_mut(sid).unwrap();
        s.insert_note(4, "First note.", ParaFormat::default());
        s.insert_note(9, "Second\nnote.", ParaFormat::default());
    }
    d.footnote_options.style = designcraft_doc::NumberStyle::LowerRoman;
    d.footnote_options.restart = designcraft_doc::notes::NoteRestart::Page;
    d.footnote_options.affix_in = designcraft_doc::notes::AffixIn::Both;
    d.footnote_options.prefix = "[".into();
    d.footnote_options.rule.width = 100.0;
    d.footnote_options.space_before = 4.0;
    d.check().unwrap();
    let bytes = export_idml(&d);
    let back = import_idml(&bytes).unwrap();
    back.check().unwrap();
    let s = back.stories.values().next().unwrap();
    assert_eq!(s.text, d.story(sid).unwrap().text);
    assert_eq!(s.notes.len(), 2);
    assert_eq!(s.notes[0].text.text, "First note.");
    assert_eq!(s.notes[1].text.text, "Second\nnote.");
    assert_eq!(back.footnote_options, d.footnote_options);
}

/// The structure InDesign writes: the footnote in its own superscript range, its text starting
/// with the number marker and the separator.
#[test]
fn imports_indesign_style_footnote() {
    let story = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
<Story Self="s1"><ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/NormalParagraphStyle">
<CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>Body text with a</Content></CharacterStyleRange>
<CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" Position="Superscript"><Footnote>
<ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/NormalParagraphStyle"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content><?ACE 4?>&#x9;A source note.</Content></CharacterStyleRange></ParagraphStyleRange>
</Footnote></CharacterStyleRange>
<CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content> note here.</Content></CharacterStyleRange>
</ParagraphStyleRange></Story></idPkg:Story>"#;
    let d = import_idml(&fixture_with_story(story)).unwrap();
    d.check().unwrap();
    let s = d.stories.values().find(|s| !s.notes.is_empty()).expect("story with a footnote");
    assert_eq!(s.text, format!("Body text with a{} note here.", designcraft_doc::FOOTNOTE_REF));
    assert_eq!(s.notes[0].text.text, "A source note.");
    // One run: the reference takes its position from the options, not an override.
    assert_eq!(s.chars.len(), 1);
}

#[test]
fn round_trips_cross_references() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) =
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(40.0, 40.0, 400.0, 400.0), lid, "Results: found\nSee .", ParaFormat::default()).unwrap();
    let target = d.paragraph_anchor(sid, 0).unwrap();
    let at = d.story(sid).unwrap().text.find(" .").unwrap() + 1;
    d.story_mut(sid).unwrap().insert_xref(at, designcraft_doc::CrossRef { target, format: "Paragraph Text".into() });
    d.xref_formats.push(designcraft_doc::XrefFormat {
        name: "Mine".into(),
        definition: "(<partialPara delim=\":\" includeDelim=\"true\" />, p. <pageNum />)".into(),
    });
    d.check().unwrap();
    let back = import_idml(&export_idml(&d)).unwrap();
    back.check().unwrap();
    let s = back.stories.values().next().unwrap();
    assert_eq!(s.text, d.story(sid).unwrap().text);
    assert_eq!(s.xrefs.len(), 1);
    assert_eq!(s.xrefs[0].format, "Paragraph Text");
    let (tsid, tpos) = back.find_anchor(s.xrefs[0].target).expect("target resolves");
    assert_eq!((tsid, tpos), (s.id, 0));
    let mine = back.xref_format("Mine").unwrap();
    assert_eq!(mine.definition, d.xref_format("Mine").unwrap().definition);
    assert_eq!(back.xref_formats.len(), d.xref_formats.len());
}

#[test]
fn round_trips_index_references() {
    use designcraft_doc::index::IndexRange;
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d
        .add_text_frame(SpreadRef::Doc(0), Rect::new(40.0, 40.0, 400.0, 400.0), lid, "Kerning adjusts pairs.\nMore.", ParaFormat::default())
        .unwrap();
    {
        let s = d.story_mut(sid).unwrap();
        s.insert_index_ref(
            0,
            designcraft_doc::IndexRef { topics: vec!["Type".into(), "Kerning".into()], sort: vec![], range: IndexRange::CurrentPage },
        );
        s.insert_index_ref(5, designcraft_doc::IndexRef { topics: vec!["Pairs".into()], sort: vec![], range: IndexRange::NextParagraphs(2) });
        s.insert_index_ref(9, designcraft_doc::IndexRef { topics: vec!["Spacing".into()], sort: vec![], range: IndexRange::See("Type".into()) });
    }
    d.check().unwrap();
    let back = import_idml(&export_idml(&d)).unwrap();
    back.check().unwrap();
    let s = back.stories.values().next().unwrap();
    let mut got: Vec<(Vec<String>, IndexRange)> = s.index_refs.iter().map(|r| (r.topics.clone(), r.range.clone())).collect();
    got.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        got,
        [
            (vec!["Pairs".to_string()], IndexRange::NextParagraphs(2)),
            (vec!["Spacing".to_string()], IndexRange::See("Type".into())),
            (vec!["Type".to_string(), "Kerning".to_string()], IndexRange::CurrentPage),
        ]
    );
    // The text survives; page references keep their places (See references, which InDesign keeps
    // on topics rather than in text, come back at the story start).
    assert_eq!(s.text.replace(designcraft_doc::INDEX_MARK, ""), "Kerning adjusts pairs.\nMore.");
    assert!(s.text.starts_with(&format!("{0}{0}Ke{0}rning", designcraft_doc::INDEX_MARK)), "{:?}", s.text);
}

#[test]
fn round_trips_anchored_objects() {
    use designcraft_doc::{AnchorPosition, AnchoredObject, anchored::AnchorAlign};
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) =
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(40.0, 40.0, 400.0, 400.0), lid, "Inline here.\nAbove.", ParaFormat::default()).unwrap();
    let mut a =
        designcraft_doc::Item::new(designcraft_doc::ItemId(d.alloc()), lid, Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 20.0, 16.0)));
    a.fill = Fill::swatch(designcraft_color::swatch::BLACK);
    let b = designcraft_doc::Item::new(designcraft_doc::ItemId(d.alloc()), lid, Shape::Oval, shapes::ellipse(Rect::new(0.0, 0.0, 60.0, 30.0)));
    {
        let s = d.story_mut(sid).unwrap();
        s.insert_object(7, AnchoredObject::new(a, AnchorPosition::Inline { y_offset: 2.0 }));
        let at = s.text.find("Above").unwrap();
        s.insert_object(at, AnchoredObject::new(b, AnchorPosition::AboveLine { align: AnchorAlign::Center, space_before: 3.0, space_after: 4.0 }));
    }
    d.check().unwrap();
    let back = import_idml(&export_idml(&d)).unwrap();
    back.check().unwrap();
    let s = back.stories.values().find(|s| !s.objects.is_empty()).unwrap();
    assert_eq!(s.text, d.story(sid).unwrap().text);
    assert_eq!(s.objects.len(), 2);
    let (w, h) = s.objects[0].size();
    assert!((w - 20.0).abs() < 1e-6 && (h - 16.0).abs() < 1e-6);
    assert_eq!(s.objects[0].position, AnchorPosition::Inline { y_offset: 2.0 });
    assert_eq!(s.objects[0].item.fill.swatch, designcraft_color::swatch::BLACK);
    assert_eq!(s.objects[1].item.shape, Shape::Oval);
    assert_eq!(s.objects[1].position, AnchorPosition::AboveLine { align: AnchorAlign::Center, space_before: 3.0, space_after: 4.0 });
}

#[test]
fn round_trips_frames_with_pasted_in_items() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let inner = designcraft_doc::Item::new(designcraft_doc::ItemId(d.alloc()), lid, Shape::Oval, shapes::ellipse(Rect::new(0.0, 0.0, 300.0, 300.0)));
    let mut frame = designcraft_doc::Item::new(
        designcraft_doc::ItemId(d.alloc()),
        lid,
        Shape::Rectangle,
        shapes::rectangle(Rect::new(100.0, 100.0, 200.0, 200.0)),
    );
    frame.content = designcraft_doc::Content::Group { items: vec![std::sync::Arc::new(inner)] };
    d.insert_item(SpreadRef::Doc(0), frame, None).unwrap();
    let back = import_idml(&export_idml(&d)).unwrap();
    let f = &back.spreads[0].items[0];
    assert_eq!(f.shape, Shape::Rectangle);
    assert!(f.has_nested_items(), "{:?}", f.content);
    assert_eq!(f.children()[0].shape, Shape::Oval);
    let b = f.bounds();
    assert!((b.x0 - 100.0).abs() < 0.01 && (b.x1 - 200.0).abs() < 0.01, "{b:?}");
}

#[test]
fn vertical_story_orientation_round_trips() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (a, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(72.0, 72.0, 200.0, 400.0), lid, "縦書き", ParaFormat::default()).unwrap();
    let (b, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(300.0, 72.0, 428.0, 400.0), lid, "", ParaFormat::default()).unwrap();
    d.thread(a, b).unwrap();
    d.add_text_frame(SpreadRef::Doc(0), Rect::new(450.0, 72.0, 550.0, 400.0), lid, "横", ParaFormat::default()).unwrap();
    d.story_mut(sid).unwrap().vertical = true;
    // Orientation and column direction are separate settings of the story.
    d.story_mut(sid).unwrap().direction = designcraft_doc::TextDirection::RightToLeft;
    let back = import_idml(&export_idml(&d)).unwrap();
    use designcraft_doc::TextDirection::{LeftToRight, RightToLeft};
    let directions: Vec<_> = back.stories.values().map(|s| (s.frames.len(), s.vertical, s.direction)).collect();
    assert!(directions.contains(&(2, true, RightToLeft)) && directions.contains(&(1, false, LeftToRight)), "{directions:?}");
    let frames: Vec<bool> = back.spreads[0].items.iter().map(|i| back.frame_vertical(i)).collect();
    assert_eq!(frames, [true, true, false]);
}

#[test]
fn cjk_character_attributes_import_from_independent_xml_and_round_trip() {
    let story = r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Story Self="s1">
      <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/[No paragraph style]" BunriKinshi="true" Rensuuji="false" TreatIdeographicSpaceAsSpace="true">
        <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" LeadingAki="0.25" TrailingAki="-1" Tsume="20" Jidori="4" LeadingModel="LeadingModelCenter" CharacterAlignment="AlignEmCenter" KentenKind="KentenWhiteCircle"><Content>甲乙</Content></CharacterStyleRange>
      </ParagraphStyleRange></Story></idPkg:Story>"#;
    let doc = import_idml(&fixture_with_story(story)).unwrap();
    let st = doc.stories.values().find(|s| s.text.contains("甲乙")).unwrap();
    let a = &st.runs().next().unwrap().1.over;
    assert_eq!(a.leading_aki, Some(Some(0.25)));
    assert_eq!(a.trailing_aki, Some(None));
    assert_eq!(a.jidori, Some(4));
    assert_eq!(a.tsume, Some(0.2));
    assert_eq!(a.kenten_character.as_deref(), Some("○"));
    assert_eq!(a.leading_model, Some(designcraft_doc::cjk::LeadingModel::Center));
    assert_eq!(st.paras[0].para.bunri_kinshi, Some(true));
    assert_eq!(st.paras[0].para.rensuuji, Some(false));
    let back = import_idml(&export_idml(&doc)).unwrap();
    let st2 = back.stories.values().find(|s| s.text.contains("甲乙")).unwrap();
    let b = &st2.runs().next().unwrap().1.over;
    assert_eq!(a.leading_aki, b.leading_aki);
    assert_eq!(a.trailing_aki, b.trailing_aki);
    assert_eq!(a.kenten_character, b.kenten_character);
    assert_eq!(a.leading_model, b.leading_model);
}

#[test]
fn warichu_imports_from_idml_and_round_trips() {
    let story = r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Story Self="s1">
      <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/[No paragraph style]">
        <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" Warichu="true" WarichuLines="3" WarichuSize="40" WarichuLineSpacing="1" WarichuAlignment="Left" WarichuCharsBeforeBreak="2" WarichuCharsAfterBreak="2"><Content>注釈</Content></CharacterStyleRange>
      </ParagraphStyleRange></Story></idPkg:Story>"#;
    let doc = import_idml(&fixture_with_story(story)).unwrap();
    let st = doc.stories.values().find(|s| s.text.contains("注釈")).unwrap();
    let a = &st.runs().next().unwrap().1.over;
    assert_eq!(a.warichu, Some(true));
    assert_eq!(a.warichu_lines, Some(3));
    assert_eq!(a.warichu_size, Some(40.0));
    assert_eq!(a.warichu_line_spacing, Some(1.0));
    assert_eq!(a.warichu_alignment, Some(designcraft_doc::cjk::WarichuAlignment::Left));
    assert_eq!(a.warichu_chars_before_break, Some(2));
    assert_eq!(a.warichu_chars_after_break, Some(2));
    let back = import_idml(&export_idml(&doc)).unwrap();
    let b = &back.stories.values().find(|s| s.text.contains("注釈")).unwrap().runs().next().unwrap().1.over;
    assert_eq!(a.warichu, b.warichu);
    assert_eq!(a.warichu_lines, b.warichu_lines);
    assert_eq!(a.warichu_size, b.warichu_size);
    assert_eq!(a.warichu_alignment, b.warichu_alignment);
    assert_eq!(a.warichu_chars_before_break, b.warichu_chars_before_break);
    assert_eq!(a.warichu_chars_after_break, b.warichu_chars_after_break);
}

#[test]
fn automatic_kerning_does_not_import_inactive_numeric_values() {
    for (attributes, expected) in [
        (r#"KerningMethod="$ID/Optical" KerningValue="1e+11""#, Some(designcraft_doc::Kerning::Optical)),
        (r#"KerningMethod="$ID/Metrics" KerningValue="0""#, Some(designcraft_doc::Kerning::Metrics)),
        (r#"KerningValue="-40""#, Some(designcraft_doc::Kerning::Manual(-40.0))),
        (r#"KerningValue="1e+11""#, None),
    ] {
        let story = format!(
            r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Story Self="s1">
        <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/[No paragraph style]">
        <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" {attributes}><Content>甲乙</Content></CharacterStyleRange>
        </ParagraphStyleRange></Story></idPkg:Story>"#
        );
        let d = import_idml(&fixture_with_story(&story)).unwrap();
        let s = d.stories.values().find(|s| s.text.contains("甲乙")).unwrap();
        assert_eq!(s.runs().next().unwrap().1.over.kerning, expected, "{attributes}");
    }
}

#[test]
fn cjk_composite_fonts_and_custom_kinsoku_are_document_resources() {
    let map = DESIGNMAP.replace("</Document>", r#"
      <KinsokuTable Self="KinsokuTable/Test" Name="Test" CantBeginLineChars="乙" CantEndLineChars="甲" CantBeSeparatedChars="—" HangingPunctuationChars="。"/>
      <CompositeFont Self="CompositeFont/Mixed" Name="Mixed"><CompositeFontEntry Self="cf1" Name="Base" FontStyle="Regular"><Properties><AppliedFont type="string">Source Serif 4</AppliedFont></Properties></CompositeFontEntry>
      <CompositeFontEntry Self="cf2" Name="Digits" CustomCharacters="0123456789" FontStyle="Regular" RelativeSize="80" BaselineShift="10"><Properties><AppliedFont type="string">Source Sans 3</AppliedFont></Properties></CompositeFontEntry></CompositeFont>
    </Document>"#);
    let story = STORY.replace("<ParagraphStyleRange ", "<ParagraphStyleRange KinsokuSet=\"KinsokuTable/Test\" ");
    let bytes = zip_files(&[
        ("designmap.xml", &map),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Styles.xml", STYLES),
        ("Resources/Preferences.xml", PREFS),
        ("MasterSpreads/MasterSpread_m1.xml", MASTER),
        ("Spreads/Spread_sp1.xml", SPREAD),
        ("Stories/Story_s1.xml", &story),
    ]);
    let d = import_idml(&bytes).unwrap();
    let f = &d.styles.composite_fonts[0];
    assert_eq!(f.entry('1').unwrap().family, "Source Sans 3");
    assert_eq!(f.entry('甲').unwrap().family, "Source Serif 4");
    assert_eq!(f.entry('1').unwrap().relative_size, 0.8);
    let back = import_idml(&export_idml(&d)).unwrap();
    assert_eq!(back.styles.composite_fonts, d.styles.composite_fonts);
    assert!(
        back.stories.values().flat_map(|s| &s.paras).any(|p| p.para.kinsoku.as_ref().and_then(Option::as_ref).is_some_and(|k| k.no_start == "乙"))
    );
}

#[test]
fn cjk_unsupported_mojikumi_is_preserved_instead_of_silently_dropped() {
    let map = DESIGNMAP.replace("</Document>", r#"<MojikumiTable Self="MojikumiTable/Spacing" Name="Spacing" BasedOnMojikumiSet="SimpChineseDefault"><Properties><OverrideMojikumiAkiList>
    <OverrideMojikumiAkiType TargetMojikumiClass="1" SideMojikumiClass="23" SideIsAfterTarget="false" Minimum="-0.1" Desired="0.25" Maximum="0.5" CompressionPriority="3" AkiDoesNotFloat="true"/>
    </OverrideMojikumiAkiList></Properties></MojikumiTable></Document>"#);
    let story =
        STORY.replace("<ParagraphStyleRange ", "<ParagraphStyleRange Mojikumi=\"MojikumiTable/Spacing\" KinsokuType=\"KinsokuPushOutFirst\" ");
    let bytes = zip_files(&[
        ("designmap.xml", &map),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Styles.xml", STYLES),
        ("Resources/Preferences.xml", PREFS),
        ("MasterSpreads/MasterSpread_m1.xml", MASTER),
        ("Spreads/Spread_sp1.xml", SPREAD),
        ("Stories/Story_s1.xml", &story),
    ]);
    let d = import_idml(&bytes).unwrap();
    let t = &d.styles.mojikumi_tables[0];
    assert_eq!(t.overrides[0].minimum, -0.1);
    assert!(t.overrides[0].does_not_float);
    let back = import_idml(&export_idml(&d)).unwrap();
    assert_eq!(back.styles.mojikumi_tables, d.styles.mojikumi_tables);
    assert!(back.stories.values().flat_map(|s| &s.paras).any(|p| p.para.mojikumi.as_deref() == Some("MojikumiTable/Spacing")));
}

#[test]
fn arabic_controls_import_independent_xml_and_round_trip() {
    let story = r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Story Self="s1">
    <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/[No paragraph style]" ParagraphDirection="RightToLeftDirection" Kashidas="KashidasOff" ParagraphJustification="NaskhJustification" ParagraphKashidaWidth="2">
    <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" CharacterDirection="LeftToRightDirection" Kashidas="KashidasOff" DiacriticPosition="OpentypePosition" XOffsetDiacritic="150" YOffsetDiacritic="-100" PositionalForm="Medial"><Properties><DigitsType type="enumeration">FarsiDigits</DigitsType></Properties><Content>بَ123</Content></CharacterStyleRange>
    </ParagraphStyleRange></Story></idPkg:Story>"#;
    let d = import_idml(&fixture_with_story(story)).unwrap();
    let st = d.stories.values().find(|s| s.text.contains('ب')).unwrap();
    assert_eq!(st.paras[0].para.kashidas, Some(false));
    assert_eq!(st.paras[0].para.arabic_justification.as_deref(), Some("NaskhJustification"));
    let a = &st.runs().next().unwrap().1.over;
    assert_eq!(a.character_direction, Some(designcraft_doc::arabic::CharacterDirection::LeftToRight));
    assert_eq!(a.allow_kashidas, Some(false));
    assert_eq!(a.diacritic_x_offset, Some(150.0));
    assert_eq!(a.diacritic_y_offset, Some(-100.0));
    assert_eq!(a.digits, Some(designcraft_doc::Digits::Farsi));
    let back = import_idml(&export_idml(&d)).unwrap();
    let b = back.stories.values().find(|s| s.text.contains('ب')).unwrap();
    let attrs = &b.runs().next().unwrap().1.over;
    assert_eq!(attrs.character_direction, a.character_direction);
    assert_eq!(attrs.diacritic_x_offset, a.diacritic_x_offset);
    assert_eq!(attrs.diacritic_y_offset, a.diacritic_y_offset);
    assert_eq!(attrs.allow_kashidas, a.allow_kashidas);
    assert_eq!(attrs.positional_form, a.positional_form);
    assert_eq!(b.paras[0].para.paragraph_kashida_width, Some(Some(2.0)));
}

#[test]
fn arabic_story_and_table_directions_survive_idml_export() {
    let mut d = Document::new(&NewDocument::default());
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 300.0, 300.0), d.default_layer(), "", ParaFormat::default()).unwrap();
    let st = d.story_mut(sid).unwrap();
    st.direction = designcraft_doc::TextDirection::RightToLeft;
    let mut t = designcraft_doc::Table::new(42, 1, 2, 0, 0, 200.0);
    t.options.direction = designcraft_doc::TextDirection::RightToLeft;
    st.insert_table(0, t);
    let back = import_idml(&export_idml(&d)).unwrap();
    let st = back.stories.values().find(|st| !st.tables.is_empty()).unwrap();
    assert_eq!(st.direction, designcraft_doc::TextDirection::RightToLeft);
    assert_eq!(st.tables.values().next().unwrap().options.direction, designcraft_doc::TextDirection::RightToLeft);
}
/// Fully synthetic style-only table, including sparse based-on attributes and local overrides.
const CASCADE_STYLES: &str = r#"<idPkg:Styles xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
<RootCellStyleGroup>
  <CellStyle Self="CellStyle/$ID/[None]" Name="$ID/[None]"/>
  <CellStyle Self="cs-base" Name="Base" FillColor="Color/Brand" FillTint="25"
    TopInset="2" LeftInset="3" BottomInset="4" RightInset="5" VerticalJustification="CenterAlign"
    TopEdgeStrokeWeight="2" TopEdgeStrokeColor="Color/Brand" TopEdgeStrokeTint="40" TopEdgeStrokeType="StrokeStyle/$ID/Dashed"
    LeftEdgeStrokeWeight="3" BottomEdgeStrokeWeight="4" RightEdgeStrokeWeight="5"
    AppliedParagraphStyle="ParagraphStyle/CellParagraph"/>
  <CellStyle Self="cs-body" Name="Body" FillTint="50"><Properties><BasedOn type="object">cs-base</BasedOn></Properties></CellStyle>
  <CellStyle Self="cs-left" Name="Left" FillColor="Color/Black" LeftEdgeStrokeWeight="0"><Properties><BasedOn type="object">cs-body</BasedOn></Properties></CellStyle>
  <CellStyle Self="cs-header" Name="Header" FillColor="Color/Paper" BottomEdgeStrokeColor="Color/Brand" BottomEdgeStrokeWeight="6">
    <Properties><BasedOn type="object">cs-body</BasedOn></Properties>
  </CellStyle>
  <CellStyle Self="cs-explicit" Name="Explicit" FillColor="Color/Brand" FillTint="75" TopInset="0"
    TopEdgeStrokeColor="Swatch/None" RightEdgeStrokeTint="30"/>
</RootCellStyleGroup>
<RootParagraphStyleGroup><ParagraphStyle Self="ParagraphStyle/CellParagraph" Name="CellParagraph" PointSize="9"/></RootParagraphStyleGroup>
<RootTableStyleGroup>
  <TableStyle Self="TableStyle/$ID/[No table style]" Name="$ID/[No table style]"
    TopBorderStrokeWeight="7" LeftBorderStrokeWeight="8" BottomBorderStrokeWeight="9" RightBorderStrokeWeight="10"
    StartRowFillCount="0" StartRowFillColor="Color/Brand" StartRowFillTint="20" EndRowFillCount="0" EndRowFillColor="Swatch/None"
    HeaderRegionSameAsBodyRegion="true" FooterRegionSameAsBodyRegion="true"
    LeftColumnRegionSameAsBodyRegion="true" RightColumnRegionSameAsBodyRegion="true"/>
  <TableStyle Self="ts-base" Name="BaseTable" BodyRegionCellStyle="cs-body" HeaderRegionCellStyle="cs-header"
    LeftColumnRegionCellStyle="cs-left" HeaderRegionSameAsBodyRegion="false" LeftColumnRegionSameAsBodyRegion="false">
    <Properties><BasedOn type="string">$ID/[No table style]</BasedOn></Properties>
  </TableStyle>
  <TableStyle Self="ts-child" Name="Table" SpaceBefore="12" StartRowFillCount="1" EndRowFillCount="1">
    <Properties><BasedOn type="object">ts-base</BasedOn></Properties>
  </TableStyle>
  <TableStyle Self="TableStyle/$ID/[Basic Table]" Name="$ID/[Basic Table]" SpaceAfter="17">
    <Properties><BasedOn type="object">ts-child</BasedOn></Properties>
  </TableStyle>
</RootTableStyleGroup></idPkg:Styles>"#;

const CASCADE_STORY: &str = r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
<Story Self="s1"><ParagraphStyleRange><CharacterStyleRange><Table AppliedTableStyle="ts-child" HeaderRowCount="1" SpaceBefore="19" RightBorderStrokeColor="Color/Brand">
<Row Name="0" MinimumHeight="24"/><Row Name="1" MinimumHeight="24"/>
<Column Name="0" SingleColumnWidth="60"/><Column Name="1" SingleColumnWidth="60"/><Column Name="2" SingleColumnWidth="60"/>
<Cell Name="0:0" AppliedCellStyle="CellStyle/$ID/[None]"><ParagraphStyleRange><CharacterStyleRange><Content>Header left</Content></CharacterStyleRange></ParagraphStyleRange></Cell>
<Cell Name="1:0" AppliedCellStyle="cs-explicit"><ParagraphStyleRange><CharacterStyleRange><Content>Explicit</Content></CharacterStyleRange></ParagraphStyleRange></Cell>
<Cell Name="2:0" AppliedCellStyle="cs-explicit" FillColor="Swatch/None" FillTint="0" LeftInset="0" VerticalJustification="BottomAlign"
  TopEdgeStrokeWeight="0" RightEdgeStrokeColor="Swatch/None" RightEdgeStrokePriority="7">
  <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/NormalParagraphStyle"><CharacterStyleRange><Content>Local</Content></CharacterStyleRange></ParagraphStyleRange>
</Cell>
<Cell Name="0:1" AppliedCellStyle="CellStyle/$ID/[None]"><ParagraphStyleRange><CharacterStyleRange><Content>Left</Content></CharacterStyleRange></ParagraphStyleRange></Cell>
<Cell Name="1:1"><ParagraphStyleRange><CharacterStyleRange><Content>Body</Content></CharacterStyleRange></ParagraphStyleRange></Cell>
<Cell Name="2:1"><ParagraphStyleRange><CharacterStyleRange><Content>Body right</Content></CharacterStyleRange></ParagraphStyleRange></Cell>
</Table></CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>"#;

fn fixture_with_table_styles(styles: &str, story: &str) -> Vec<u8> {
    zip_files(&[
        ("designmap.xml", DESIGNMAP),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Styles.xml", styles),
        ("Resources/Preferences.xml", PREFS),
        ("MasterSpreads/MasterSpread_m1.xml", MASTER),
        ("Spreads/Spread_sp1.xml", SPREAD),
        ("Stories/Story_s1.xml", story),
    ])
}

#[test]
fn table_style_cascade_preserves_sparse_edges_and_local_overrides() {
    let d = import_idml(&fixture_with_table_styles(CASCADE_STYLES, CASCADE_STORY)).unwrap();
    d.check().unwrap();
    let table = d.stories.values().flat_map(|s| s.tables.values()).next().unwrap();
    assert_eq!((table.nrows(), table.ncols()), (2, 3));
    assert_eq!(table.style, "Table");
    assert_eq!(table.options.space_before, 19.0);
    assert_eq!(std::array::from_fn::<_, 4, _>(|i| table.options.border_for(i).weight), [7.0, 8.0, 9.0, 10.0]);
    assert_eq!(table.options.border_for(3).color, "Brand");
    let fills = table.options.alt_rows.as_ref().unwrap();
    assert_eq!((fills.first, fills.next, fills.first_tint), (1, 1, 0.2));
    assert_eq!(fills.first_color, "Brand");
    let header = table.cell(0, 0).unwrap();
    assert_eq!(header.fill, designcraft_color::swatch::PAPER, "header takes precedence over left region");
    assert!(header.style.is_empty(), "a table region does not become AppliedCellStyle");
    assert_eq!(header.insets, [2.0, 3.0, 4.0, 5.0]);
    assert_eq!(header.vj, designcraft_doc::VerticalJustification::Center);
    assert_eq!(header.text.paras[0].style, "CellParagraph");
    assert_eq!(header.strokes[0].weight, 2.0);
    assert_eq!(header.strokes[0].tint, 0.4);
    assert_ne!(header.strokes[0].kind, designcraft_doc::StrokeType::Solid);
    assert_eq!(header.strokes[2].weight, 6.0);
    assert_eq!(header.border_overrides, [true; 4]);
    assert_eq!(header.stroke_defined, [true; 4]);
    assert_eq!(header.stroke_priorities, [0; 4]);
    let explicit = table.cell(0, 1).unwrap();
    assert_eq!(explicit.fill, "Brand");
    assert_eq!(explicit.fill_tint, 0.75);
    assert_eq!(explicit.insets, [0.0, 3.0, 4.0, 5.0]);
    assert_eq!(explicit.strokes[0].weight, 2.0, "color-only override inherits weight");
    assert_eq!(explicit.strokes[0].color, designcraft_color::swatch::NONE);
    assert_eq!(explicit.strokes[3].weight, 5.0);
    assert_eq!(explicit.strokes[3].tint, 0.3);
    let local = table.cell(0, 2).unwrap();
    assert_eq!(local.fill, designcraft_color::swatch::NONE);
    assert_eq!(local.fill_tint, 0.0);
    assert_eq!(local.insets[1], 0.0);
    assert_eq!(local.vj, designcraft_doc::VerticalJustification::Bottom);
    assert_eq!(local.strokes[0].weight, 0.0);
    assert_eq!(local.strokes[3].color, designcraft_color::swatch::NONE);
    assert_eq!(local.stroke_priorities, [0, 0, 0, 7]);
    assert_eq!(local.text.paras[0].style, st::BASIC_PARAGRAPH, "explicit paragraph style wins");
    assert_eq!(table.cell(1, 0).unwrap().fill, designcraft_color::swatch::BLACK);
    assert_eq!(table.cell(1, 0).unwrap().strokes[1].weight, 0.0);
    assert_eq!(table.cell(1, 1).unwrap().fill_tint, 0.5);
    assert_eq!(table.cell(1, 2).unwrap().fill, "Brand");

    let back = import_idml(&export_idml(&d)).unwrap();
    let roundtrip = back.stories.values().flat_map(|s| s.tables.values()).next().unwrap();
    for (before, after) in table.cells.iter().zip(&roundtrip.cells) {
        assert_eq!(after.strokes, before.strokes);
        assert_eq!(after.border_overrides, before.border_overrides);
        assert_eq!(after.stroke_priorities, before.stroke_priorities);
        assert_eq!(after.style, before.style);
        assert_eq!(after.fill, before.fill);
        assert_eq!(after.insets, before.insets);
    }
    for i in 0..4 {
        assert_eq!(roundtrip.options.border_for(i), table.options.border_for(i));
    }
    let style = back.styles.cell.iter().find(|s| s.name == "Explicit").unwrap();
    assert_eq!(style.strokes[0].weight, None, "style export keeps absence distinct from zero");
    assert_eq!(style.strokes[0].color.as_deref(), Some(designcraft_color::swatch::NONE));
    assert_eq!(back.styles.table.iter().find(|s| s.name == "Table").unwrap().based_on.as_deref(), Some("BaseTable"));
}

#[test]
fn table_style_same_as_body_flags_and_editable_basic_table_are_inherited() {
    let styles = CASCADE_STYLES.replace(
        "Name=\"Table\" SpaceBefore",
        "Name=\"Table\" HeaderRegionSameAsBodyRegion=\"true\" LeftColumnRegionSameAsBodyRegion=\"true\" SpaceBefore",
    );
    let story = CASCADE_STORY.replace("AppliedTableStyle=\"ts-child\"", "AppliedTableStyle=\"TableStyle/$ID/[Basic Table]\"");
    let d = import_idml(&fixture_with_table_styles(&styles, &story)).unwrap();
    let table = d.stories.values().flat_map(|s| s.tables.values()).next().unwrap();
    assert!(table.style.is_empty());
    assert_eq!(table.options.space_after, 17.0);
    assert_eq!(table.cell(0, 0).unwrap().fill, "Brand");
    assert_eq!(table.cell(0, 0).unwrap().strokes[2].weight, 4.0);
    assert_eq!(table.cell(1, 0).unwrap().fill, "Brand");
    assert_eq!(table.cell(1, 0).unwrap().strokes[1].weight, 3.0);
}

#[test]
fn table_style_cycles_and_missing_bases_terminate_without_losing_nearest_values() {
    let styles = r#"<idPkg:Styles xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
<RootCellStyleGroup>
<CellStyle Self="CellStyle/A" Name="A" TopEdgeStrokeWeight="0"><Properties><BasedOn type="object">CellStyle/B</BasedOn></Properties></CellStyle>
<CellStyle Self="CellStyle/B" Name="B" TopEdgeStrokeColor="Color/Brand"><Properties><BasedOn type="object">CellStyle/A</BasedOn></Properties></CellStyle>
<CellStyle Self="CellStyle/Missing" Name="Missing" LeftInset="0"><Properties><BasedOn type="object">CellStyle/Unknown</BasedOn></Properties></CellStyle>
</RootCellStyleGroup><RootTableStyleGroup>
<TableStyle Self="ts-child" Name="A" BodyRegionCellStyle="CellStyle/A"><Properties><BasedOn type="object">TableStyle/B</BasedOn></Properties></TableStyle>
<TableStyle Self="TableStyle/B" Name="B" SpaceAfter="20"><Properties><BasedOn type="object">ts-child</BasedOn></Properties></TableStyle>
</RootTableStyleGroup></idPkg:Styles>"#;
    let d = import_idml(&fixture_with_table_styles(styles, CASCADE_STORY)).unwrap();
    let table = d.stories.values().flat_map(|s| s.tables.values()).next().unwrap();
    assert_eq!(table.options.space_after, 20.0);
    assert_eq!(table.cell(1, 1).unwrap().strokes[0].weight, 0.0);
    assert_eq!(table.cell(1, 1).unwrap().strokes[0].color, "Brand");
    let missing = designcraft_doc::resolve_cell_style(&d.styles.cell, "Missing");
    assert_eq!(missing.inset_overrides[1], Some(0.0));
}

#[test]
fn table_local_alternating_fill_off_survives_named_style_roundtrip() {
    let styles = CASCADE_STYLES.replace(
        "Name=\"Table\" SpaceBefore",
        "Name=\"Table\" StartColumnFillCount=\"1\" EndColumnFillCount=\"1\" StartColumnFillColor=\"Color/Brand\" SpaceBefore",
    );
    let story = CASCADE_STORY.replace("HeaderRowCount=\"1\"", "HeaderRowCount=\"1\" StartRowFillCount=\"0\" StartColumnFillCount=\"0\"");
    let document = import_idml(&fixture_with_table_styles(&styles, &story)).unwrap();
    for document in [&document, &import_idml(&export_idml(&document)).unwrap()] {
        let table = document.stories.values().flat_map(|s| s.tables.values()).next().unwrap();
        assert!(table.options.alt_rows.is_none());
        assert!(table.options.alt_cols.is_none());
    }
}

#[test]
fn table_local_fill_counts_enable_inherited_disabled_pattern_colors() {
    let story =
        CASCADE_STORY.replace("AppliedTableStyle=\"ts-child\"", "AppliedTableStyle=\"ts-base\" StartRowFillCount=\"1\" EndRowFillCount=\"1\"");
    let document = import_idml(&fixture_with_table_styles(CASCADE_STYLES, &story)).unwrap();
    let table = document.stories.values().flat_map(|s| s.tables.values()).next().unwrap();
    let fills = table.options.alt_rows.as_ref().unwrap();
    assert_eq!((fills.first, fills.next), (1, 1));
    assert_eq!(fills.first_color, "Brand");
    assert_eq!(fills.first_tint, 0.2);
}

#[test]
fn table_absent_and_explicit_default_edges_remain_distinct_on_idml_roundtrip() {
    let story = CASCADE_STORY
        .replace("AppliedTableStyle=\"ts-child\"", "AppliedTableStyle=\"Missing\"")
        .replace("Name=\"1:1\"", "Name=\"1:1\" TopEdgeStrokeWeight=\"1\" TopEdgeStrokePriority=\"0\"");
    let document = import_idml(&fixture_with_table_styles(STYLES, &story)).unwrap();
    for document in [&document, &import_idml(&export_idml(&document)).unwrap()] {
        let table = document.stories.values().flat_map(|s| s.tables.values()).next().unwrap();
        assert_eq!(table.cell(1, 0).unwrap().stroke_defined, [false; 4]);
        assert_eq!(table.cell(1, 1).unwrap().stroke_defined, [true, false, false, false]);
        assert_eq!(table.cell(1, 1).unwrap().border_overrides, [false; 4]);
        assert_eq!(table.cell(1, 1).unwrap().stroke_priorities, [0; 4]);
    }
}

#[test]
fn multiline_table_and_cell_style_bases_resolve() {
    let styles = CASCADE_STYLES.replace(">cs-base</BasedOn>", ">\n  cs-base \n</BasedOn>").replace(">ts-base</BasedOn>", ">\n ts-base \n</BasedOn>");
    let document = import_idml(&fixture_with_table_styles(&styles, CASCADE_STORY)).unwrap();
    let table = document.stories.values().flat_map(|s| s.tables.values()).next().unwrap();
    assert_eq!(table.cell(1, 1).unwrap().strokes[0].weight, 2.0);
    assert_eq!(table.options.border_for(0).weight, 7.0);
}

#[test]
fn legacy_native_documents_export_both_builtin_table_styles_when_missing() {
    let story = CASCADE_STORY.replace("AppliedTableStyle=\"ts-child\"", "AppliedTableStyle=\"TableStyle/$ID/[Basic Table]\"");
    let mut document = import_idml(&fixture_with_story(&story)).unwrap();
    document.styles_mut().table.clear();
    let bytes = export_idml(&document);
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    let mut styles = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("Resources/Styles.xml").unwrap(), &mut styles).unwrap();
    assert_eq!(styles.matches("Self=\"TableStyle/$ID/[Basic Table]\"").count(), 1);
    assert_eq!(styles.matches("Self=\"TableStyle/$ID/[No table style]\"").count(), 1);
    let back = import_idml(&bytes).unwrap();
    assert!(back.styles.table.iter().any(|s| s.name == designcraft_doc::BASIC_TABLE));
}

#[test]
fn hostile_table_spans_are_clamped_before_arithmetic() {
    let story = CASCADE_STORY.replace("Name=\"1:1\"", "Name=\"1:1\" RowSpan=\"inf\" ColumnSpan=\"inf\"");
    let document = import_idml(&fixture_with_table_styles(CASCADE_STYLES, &story)).unwrap();
    document.check().unwrap();
    let table = document.stories.values().flat_map(|s| s.tables.values()).next().unwrap();
    assert_eq!(table.cell(1, 1).unwrap().row_span, 1);
    assert_eq!(table.cell(1, 1).unwrap().col_span, 2);
}

#[test]
fn signed_idml_edge_priorities_roundtrip_without_clamping() {
    let story = CASCADE_STORY.replace(
        "Name=\"1:1\"",
        "Name=\"1:1\" TopEdgeStrokePriority=\"-5\" LeftEdgeStrokePriority=\"-2147483648\" RightEdgeStrokePriority=\"2147483647\"",
    );
    let document = import_idml(&fixture_with_table_styles(CASCADE_STYLES, &story)).unwrap();
    for document in [&document, &import_idml(&export_idml(&document)).unwrap()] {
        let table = document.stories.values().flat_map(|s| s.tables.values()).next().unwrap();
        assert_eq!(table.cell(1, 1).unwrap().stroke_priorities, [-5, i32::MIN, 0, i32::MAX]);
    }
}

#[test]
fn alternating_table_strokes_inherit_and_roundtrip_without_flattening_cell_edges() {
    let styles = CASCADE_STYLES.replace(
        "Name=\"BaseTable\"",
        "Name=\"BaseTable\" StartRowStrokeCount=\"1\" EndRowStrokeCount=\"1\" StartRowStrokeWeight=\"0\" EndRowStrokeWeight=\"0\" StartRowStrokeColor=\"Color/Paper\" EndRowStrokeColor=\"Color/Brand\" StartColumnStrokeCount=\"2\" EndColumnStrokeCount=\"1\" StartColumnStrokeWeight=\"1\" EndColumnStrokeWeight=\"2\" EndColumnLineStyle=\"StrokeStyle/$ID/Dotted\" SkipFirstAlternatingStrokeColumns=\"1\"",
    ).replace("Name=\"Table\"", "Name=\"Table\" StartRowStrokeTint=\"25\"");
    let story = CASCADE_STORY.replace("HeaderRowCount=\"1\"", "HeaderRowCount=\"1\" EndRowStrokeColor=\"Swatch/None\"");
    let document = import_idml(&fixture_with_table_styles(&styles, &story)).unwrap();
    let before = document.stories.values().flat_map(|s| s.tables.values()).next().unwrap();
    let row = before.options.row_strokes.as_ref().unwrap();
    assert_eq!((row.first, row.next), (1, 1));
    assert_eq!(row.first_stroke.tint, 0.25);
    assert_eq!(row.first_stroke.color, designcraft_color::swatch::PAPER);
    assert_eq!(row.next_stroke.color, designcraft_color::swatch::NONE);
    assert!(!row.uniform_stroke().unwrap().is_visible(), "zero patterns are uniform even when their colors differ");
    let column = before.options.column_strokes.as_ref().unwrap();
    assert_eq!((column.first, column.next, column.skip_first), (2, 1, 1));
    assert_eq!(column.next_stroke.kind, designcraft_doc::StrokeType::Dotted);
    assert!(column.uniform_stroke().is_none(), "nonuniform/skipped patterns are preserved without guessing their phase");
    assert_eq!(before.cell(0, 0).unwrap().strokes[2].weight, 6.0, "cell styles remain higher-precedence edges");
    let back = import_idml(&export_idml(&document)).unwrap();
    let after = back.stories.values().flat_map(|s| s.tables.values()).next().unwrap();
    assert_eq!(after.options.row_strokes, before.options.row_strokes);
    assert_eq!(after.options.column_strokes, before.options.column_strokes);
    let style = back.styles.table.iter().find(|s| s.name == "Table").unwrap();
    assert_eq!(style.row_strokes.first_stroke.weight, None);
    assert_eq!(style.row_strokes.first_stroke.tint, Some(0.25));
}

#[test]
fn alternating_table_strokes_local_off_overrides_named_style_on_roundtrip() {
    let styles = CASCADE_STYLES.replace(
        "Name=\"Table\"",
        "Name=\"Table\" StartRowStrokeCount=\"1\" EndRowStrokeCount=\"1\" StartColumnStrokeCount=\"1\" EndColumnStrokeCount=\"1\"",
    );
    let story = CASCADE_STORY.replace(
        "HeaderRowCount=\"1\"",
        "HeaderRowCount=\"1\" StartRowStrokeCount=\"0\" EndRowStrokeCount=\"0\" StartRowStrokeWeight=\"4\" EndRowStrokeColor=\"Color/Brand\" StartColumnStrokeCount=\"0\" EndColumnStrokeCount=\"0\" SkipFirstAlternatingStrokeColumns=\"3\" EndColumnLineStyle=\"StrokeStyle/$ID/Dotted\"",
    );
    let document = import_idml(&fixture_with_table_styles(&styles, &story)).unwrap();
    for document in [&document, &import_idml(&export_idml(&document)).unwrap()] {
        let table = document.stories.values().flat_map(|s| s.tables.values()).next().unwrap();
        let row = table.options.row_strokes.as_ref().unwrap();
        let column = table.options.column_strokes.as_ref().unwrap();
        assert_eq!((row.first, row.next, column.first, column.next), (0, 0, 0, 0));
        assert!(row.uniform_stroke().is_none());
        assert!(column.uniform_stroke().is_none());
        assert_eq!(row.first_stroke.weight, 4.0);
        assert_eq!(row.next_stroke.color, "Brand");
        assert_eq!(column.skip_first, 3);
        assert_eq!(column.next_stroke.kind, designcraft_doc::StrokeType::Dotted);
    }
}

#[test]
fn complete_uniform_native_style_fields_keep_legacy_identity_on_idml_roundtrip() {
    let mut document = import_idml(&fixture()).unwrap();
    let stroke = designcraft_doc::CellStroke { weight: 3.0, tint: 0.5, ..Default::default() };
    document.styles_mut().cell.push(designcraft_doc::CellStyle {
        name: "Uniform".into(),
        insets: Some([5.0; 4]),
        stroke: Some(stroke.clone()),
        ..Default::default()
    });
    document.styles_mut().table.push(designcraft_doc::TableStyle { name: "Uniform".into(), border: Some(stroke.clone()), ..Default::default() });
    let back = import_idml(&export_idml(&document)).unwrap();
    let cell = back.styles.cell.iter().find(|s| s.name == "Uniform").unwrap();
    assert_eq!(cell.insets, Some([5.0; 4]));
    assert_eq!(cell.stroke.as_ref(), Some(&stroke));
    assert!(cell.strokes.iter().all(designcraft_doc::CellStrokeAttrs::is_empty));
    let table = back.styles.table.iter().find(|s| s.name == "Uniform").unwrap();
    assert_eq!(table.border.as_ref(), Some(&stroke));
    assert!(table.borders.iter().all(designcraft_doc::CellStrokeAttrs::is_empty));
}

#[test]
fn imports_column_rules_from_frames_and_object_styles() {
    let rule = r#"ColumnRuleOverride="true" ColumnRuleStrokeWidth="2.5" ColumnRuleStrokeColor="Color/Black" ColumnRuleStrokeTint="40"
        ColumnRuleStrokeType="StrokeStyle/$ID/Solid" ColumnRuleOverprintOverride="false" ColumnRuleOffset="-3" ColumnRuleTopInset="6"
        ColumnRuleBottomInset="9" ColumnRuleInsetChainOverride="false""#;
    let styles = format!(
        r#"<ObjectStyle Self="ObjectStyle/Padded" Name="Padded" EnableTextFrameGeneralOptions="true"><TextFramePreference TextColumnCount="2" {rule}/></ObjectStyle>"#
    );
    let d = inset_fixture(&format!(r#"<TextFramePreference TextColumnCount="3" {rule}/>"#), &styles);
    let style = d.styles.object.iter().find(|s| s.name == "Padded").unwrap().text_frame.clone().unwrap();
    for o in [d.spreads[0].items[0].text_frame().unwrap().options.clone(), style] {
        assert!(o.column_rule, "{o:?}");
        assert_eq!(o.column_rule_weight, 2.5);
        assert_eq!(o.column_rule_color, designcraft_color::swatch::BLACK);
        assert!((o.column_rule_tint - 0.4).abs() < 1e-6);
        assert_eq!((o.column_rule_offset, o.column_rule_top_inset, o.column_rule_bottom_inset), (-3.0, 6.0, 9.0));
    }
    // Absent attributes: no rule, InDesign's defaults.
    let d = inset_fixture(r#"<TextFramePreference TextColumnCount="3"/>"#, "");
    assert_eq!(d.spreads[0].items[0].text_frame().unwrap().options, designcraft_doc::TextFrameOptions { columns: 3, ..Default::default() });
}

#[test]
fn column_rules_round_trip() {
    let mut d = small_doc();
    let fid = d.spreads[0].items[0].id;
    let o = &mut d.item_mut(fid).unwrap().text_frame_mut().unwrap().options;
    o.columns = 2;
    o.column_rule = true;
    o.column_rule_weight = 0.75;
    o.column_rule_color = "Brand".into();
    o.column_rule_tint = 0.5;
    o.column_rule_offset = 1.5;
    o.column_rule_top_inset = 4.0;
    o.column_rule_bottom_inset = 2.0;
    let want = o.clone();
    let bytes = export_idml(&d);
    let back = import_idml(&bytes).unwrap();
    let got = back.spreads[0].items.iter().find_map(|i| i.text_frame().filter(|t| t.options.column_rule)).unwrap().options.clone();
    assert_eq!(got, want);
    // Frames without a rule stay without one.
    let plain = export_idml(&small_doc());
    let back = import_idml(&plain).unwrap();
    assert!(back.spreads.iter().flat_map(|s| &s.items).filter_map(|i| i.text_frame()).all(|t| !t.options.column_rule));
}

/// InDesign's designmap lists layers back to front; `Document::layers[0]` is the front layer.
#[test]
fn layers_import_and_export_in_stacking_order() {
    let designmap = r#"<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" Self="d">
      <Layer Self="L1" Name="Background"/>
      <Layer Self="L2" Name="Content"/>
      <idPkg:Spread src="Spreads/Spread_s.xml"/>
    </Document>"#;
    let spread = r#"<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
      <Spread Self="s">
        <Page Self="p" GeometricBounds="0 0 100 100" ItemTransform="1 0 0 1 0 0"/>
        <Rectangle Self="r" ItemLayer="L1">
          <Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
            <PathPointType Anchor="0 0"/><PathPointType Anchor="0 100"/>
            <PathPointType Anchor="100 100"/><PathPointType Anchor="100 0"/>
          </PathPointArray></GeometryPathType></PathGeometry></Properties>
        </Rectangle>
      </Spread>
    </idPkg:Spread>"#;
    let d = import_idml(&zip_files(&[("designmap.xml", designmap), ("Spreads/Spread_s.xml", spread)])).unwrap();
    let names = |d: &Document| d.layers.iter().map(|l| l.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&d), ["Content", "Background"]);
    assert_eq!(d.spreads[0].items[0].layer, d.layers[1].id, "the rectangle stays on Background");

    let bytes = export_idml(&d);
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.as_slice())).unwrap();
    let mut map = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("designmap.xml").unwrap(), &mut map).unwrap();
    let (bg, content) = (map.find(r#"Name="Background""#).unwrap(), map.find(r#"Name="Content""#).unwrap());
    assert!(bg < content, "designmap lists the back layer first");
    let back = import_idml(&bytes).unwrap();
    assert_eq!(names(&back), ["Content", "Background"]);
    assert_eq!(back.spreads[0].items[0].layer, back.layers[1].id);
}

#[test]
fn split_columns_import_and_round_trip() {
    let story = r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Story Self="s1">
      <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/[No paragraph style]" SpanColumnType="SplitColumns" SplitColumnInsideGutter="27" SplitColumnOutsideGutter="4.5">
        <Properties><SpanSplitColumnCount type="short">3</SpanSplitColumnCount></Properties>
        <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>Split list</Content></CharacterStyleRange>
      </ParagraphStyleRange></Story></idPkg:Story>"#;
    let doc = import_idml(&fixture_with_story(story)).unwrap();
    let para = |d: &Document| d.stories.values().find(|s| s.text.contains("Split list")).unwrap().paras[0].para.clone();
    let a = para(&doc);
    assert_eq!(a.span_columns, Some(designcraft_doc::SpanColumns::Split(3)));
    assert_eq!(a.split_inside_gutter, Some(27.0));
    assert_eq!(a.split_outside_gutter, Some(4.5));
    let b = para(&import_idml(&export_idml(&doc)).unwrap());
    assert_eq!((b.span_columns, b.split_inside_gutter, b.split_outside_gutter), (a.span_columns, a.split_inside_gutter, a.split_outside_gutter));
}

#[test]
fn list_numbering_format_expression_and_bullet_import_and_round_trip() {
    let styles = STYLES.replace(
        "    </ParagraphStyleGroup>",
        r#"      <ParagraphStyle Self="ParagraphStyle/Text%3aHead" Name="Text:Head" BulletsAndNumberingListType="NumberedList" NumberingExpression="^#.^t">
        <Properties><NumberingFormat type="string">A, B, C, D...</NumberingFormat></Properties>
      </ParagraphStyle>
      <ParagraphStyle Self="ParagraphStyle/Text%3aTable" Name="Text:Table" BulletsAndNumberingListType="NumberedList" NumberingExpression="Tabel ^#^t">
        <Properties><NumberingFormat type="string">001, 002, 003...</NumberingFormat></Properties>
      </ParagraphStyle>
      <ParagraphStyle Self="ParagraphStyle/Text%3aKanji" Name="Text:Kanji" BulletsAndNumberingListType="NumberedList">
        <Properties><NumberingFormat type="string">一, 二, 三, 四...</NumberingFormat></Properties>
      </ParagraphStyle>
      <ParagraphStyle Self="ParagraphStyle/Text%3aPoint" Name="Text:Point" BulletsAndNumberingListType="BulletList" BulletsTextAfter="^&gt;">
        <Properties><BulletChar BulletCharacterType="UnicodeOnly" BulletCharacterValue="9632"/></Properties>
      </ParagraphStyle>
    </ParagraphStyleGroup>"#,
    );
    let bytes = zip_files(&[
        ("designmap.xml", DESIGNMAP),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Styles.xml", &styles),
        ("Resources/Preferences.xml", PREFS),
        ("MasterSpreads/MasterSpread_m1.xml", MASTER),
        ("Spreads/Spread_sp1.xml", SPREAD),
        ("Stories/Story_s1.xml", STORY),
    ]);
    let check = |d: &Document| {
        let (head, _) = d.styles.resolve_para_style("Text/Head");
        assert_eq!(head.number_style, designcraft_doc::NumberStyle::UpperLetters);
        assert_eq!(head.number_label(2), "B.\t");
        let (table, _) = d.styles.resolve_para_style("Text/Table");
        assert_eq!(table.number_label(7), "Tabel 007\t");
        let (kanji, _) = d.styles.resolve_para_style("Text/Kanji");
        assert_eq!(kanji.number_style, designcraft_doc::NumberStyle::Arabic);
        let (point, _) = d.styles.resolve_para_style("Text/Point");
        assert_eq!(point.bullet_label(), "\u{25A0}\u{2002}");
    };
    let d = import_idml(&bytes).unwrap();
    check(&d);
    check(&import_idml(&export_idml(&d)).unwrap());
}

fn fixture_with_styles(styles: &str) -> Vec<u8> {
    zip_files(&[
        ("designmap.xml", DESIGNMAP),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Styles.xml", styles),
        ("Resources/Preferences.xml", PREFS),
        ("MasterSpreads/MasterSpread_m1.xml", MASTER),
        ("Spreads/Spread_sp1.xml", SPREAD),
        ("Stories/Story_s1.xml", STORY),
    ])
}

/// Paragraph styles `Body` (based on the root) and `Loose` (no BasedOn), both with `attrs`.
fn keep_styles(attrs: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<idPkg:Styles xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <RootParagraphStyleGroup Self="rp">
    <ParagraphStyle Self="ParagraphStyle/$ID/[No paragraph style]" Name="$ID/[No paragraph style]" PointSize="12"/>
    <ParagraphStyle Self="ParagraphStyle/Body" Name="Body" {attrs}>
      <Properties><BasedOn type="object">ParagraphStyle/$ID/[No paragraph style]</BasedOn></Properties>
    </ParagraphStyle>
    <ParagraphStyle Self="ParagraphStyle/Loose" Name="Loose" {attrs}/>
  </RootParagraphStyleGroup>
</idPkg:Styles>"#
    )
}

/// Keep Lines Together whose mode no style sets is InDesign's default: at start/end of paragraph.
/// An explicit mode still wins.
#[test]
fn keep_lines_mode_defaults_to_start_and_end() {
    let keep = r#"KeepLinesTogether="true" KeepFirstLines="2" KeepLastLines="2""#;
    for (attrs, all) in [(keep.to_string(), false), (format!(r#"{keep} KeepAllLinesTogether="true""#), true)] {
        let d = import_idml_with(&fixture_with_styles(&keep_styles(&attrs)), &|_| None).unwrap();
        for name in ["Body", "Loose"] {
            let (pp, _) = d.styles.resolve_para_style(name);
            assert_eq!((pp.keep_lines_together, pp.keep_all_lines, pp.keep_first, pp.keep_last), (true, all, 2, 2), "{name}: {attrs}");
        }
    }
}

type Resolved = (designcraft_doc::ParaProps, designcraft_doc::CharProps);

/// (IDML attribute, InDesign's default as IDML writes it, another value, the resolved field,
/// its value for InDesign's default, its value for the other value).
type AttrRow = (&'static str, &'static str, &'static str, fn(&Resolved) -> String, String, String);

macro_rules! attr_rows {
    ($($attr:literal: $default:literal => $dv:expr, $other:literal => $ov:expr, |$r:ident| $field:expr;)*) => {
        vec![$(($attr, $default, $other, (|$r: &Resolved| format!("{:?}", $field)) as fn(&Resolved) -> String, format!("{:?}", $dv), format!("{:?}", $ov)),)*]
    };
}

/// InDesign's defaults for a new document's [No Paragraph Style], for every paragraph and
/// character attribute the importer reads (except the font family: see below).
fn indesign_text_defaults() -> Vec<AttrRow> {
    use designcraft_doc::{Align, Capitalization, Composer, Digits, GridAlign, Leading, ListType, Position, StartParagraph, TextDirection};
    attr_rows! {
        "Justification": "LeftAlign" => Align::Left, "CenterAlign" => Align::Center, |r| r.0.align;
        "ParagraphDirection": "LeftToRightDirection" => TextDirection::LeftToRight, "RightToLeftDirection" => TextDirection::RightToLeft, |r| r.0.direction;
        "LeftIndent": "0" => 0.0, "6" => 6.0, |r| r.0.left_indent;
        "RightIndent": "0" => 0.0, "6" => 6.0, |r| r.0.right_indent;
        "FirstLineIndent": "0" => 0.0, "6" => 6.0, |r| r.0.first_line_indent;
        "LastLineIndent": "0" => 0.0, "6" => 6.0, |r| r.0.last_line_indent;
        "SpaceBefore": "0" => 0.0, "6" => 6.0, |r| r.0.space_before;
        "SpaceAfter": "0" => 0.0, "6" => 6.0, |r| r.0.space_after;
        "DropCapLines": "0" => 0, "3" => 3, |r| r.0.drop_cap_lines;
        "DropCapCharacters": "0" => 0, "1" => 1, |r| r.0.drop_cap_chars;
        "DropcapDetail": "1" => (true, false), "2" => (false, true), |r| (r.0.drop_cap_align_left, r.0.drop_cap_scale_descenders);
        "GridAlignment": "None" => GridAlign::None, "AlignToBaseline" => GridAlign::AllLines, |r| r.0.grid_align;
        "Composer": "HL Composer" => Composer::Paragraph, "HL Single" => Composer::SingleLine, |r| r.0.composer;
        "Hyphenation": "true" => true, "false" => false, |r| r.0.hyphenate;
        "HyphenateWordsLongerThan": "5" => 5, "7" => 7, |r| r.0.hyph_min_word;
        "HyphenateAfterFirst": "2" => 2, "3" => 3, |r| r.0.hyph_after_first;
        "HyphenateBeforeLast": "2" => 2, "3" => 3, |r| r.0.hyph_before_last;
        "HyphenateLadderLimit": "3" => 3, "0" => 0, |r| r.0.hyph_limit;
        "HyphenationZone": "36" => 36.0, "18" => 18.0, |r| r.0.hyph_zone;
        "HyphenateCapitalizedWords": "true" => true, "false" => false, |r| r.0.hyph_capitalized;
        "HyphenateLastWord": "true" => true, "false" => false, |r| r.0.hyph_last_word;
        "HyphenateAcrossColumns": "true" => true, "false" => false, |r| r.0.hyph_across_column;
        "HyphenWeight": "5" => 0.5, "9" => 0.9, |r| r.0.hyph_weight;
        "MinimumWordSpacing": "80" => 0.8, "70" => 0.7, |r| r.0.word_space_min;
        "DesiredWordSpacing": "100" => 1.0, "90" => 0.9, |r| r.0.word_space_desired;
        "MaximumWordSpacing": "133" => 1.33, "150" => 1.5, |r| r.0.word_space_max;
        "MinimumLetterSpacing": "0" => 0.0, "-5" => -0.05, |r| r.0.letter_space_min;
        "DesiredLetterSpacing": "0" => 0.0, "1" => 0.01, |r| r.0.letter_space_desired;
        "MaximumLetterSpacing": "0" => 0.0, "5" => 0.05, |r| r.0.letter_space_max;
        "MinimumGlyphScaling": "100" => 1.0, "97" => 0.97, |r| r.0.glyph_scale_min;
        "DesiredGlyphScaling": "100" => 1.0, "101" => 1.01, |r| r.0.glyph_scale_desired;
        "MaximumGlyphScaling": "100" => 1.0, "103" => 1.03, |r| r.0.glyph_scale_max;
        "AutoLeading": "120" => 1.2, "100" => 1.0, |r| r.0.auto_leading;
        "SingleWordJustification": "FullyJustified" => Align::FullyJustified, "LeftAlign" => Align::Left, |r| r.0.single_word_justify;
        "KeepWithNext": "0" => 0, "1" => 1, |r| r.0.keep_with_next;
        "KeepLinesTogether": "false" => false, "true" => true, |r| r.0.keep_lines_together;
        "KeepAllLinesTogether": "false" => false, "true" => true, |r| r.0.keep_all_lines;
        "KeepFirstLines": "2" => 2, "3" => 3, |r| r.0.keep_first;
        "KeepLastLines": "2" => 2, "3" => 3, |r| r.0.keep_last;
        "StartParagraph": "Anywhere" => StartParagraph::Anywhere, "NextColumn" => StartParagraph::NextColumn, |r| r.0.start_paragraph;
        "BulletsAndNumberingListType": "NoList" => ListType::None, "BulletList" => ListType::Bullets, |r| r.0.list_type;
        "BalanceRaggedLines": "NoBalancing" => false, "FullyBalanced" => true, |r| r.0.balance_ragged;
        "Kashidas": "DefaultKashidas" => true, "KashidasOff" => false, |r| r.0.kashidas;
        "RuleAbove": "false" => false, "true" => true, |r| r.0.rule_above.on;
        "RuleAboveColor": "Text Color" => designcraft_doc::TEXT_COLOR, "Color/Black" => "[Black]", |r| r.0.rule_above.color;
        "RuleAboveTint": "-1" => 1.0f32, "50" => 0.5f32, |r| r.0.rule_above.tint;
        "RuleBelow": "false" => false, "true" => true, |r| r.0.rule_below.on;
        "RuleBelowColor": "Text Color" => designcraft_doc::TEXT_COLOR, "Color/Black" => "[Black]", |r| r.0.rule_below.color;
        "RuleBelowTint": "-1" => 1.0f32, "50" => 0.5f32, |r| r.0.rule_below.tint;
        "ParagraphShadingOn": "false" => false, "true" => true, |r| r.0.shading_on;
        "ParagraphShadingColor": "Color/Black" => "[Black]", "Swatch/None" => "[None]", |r| r.0.shading_color;
        "ParagraphShadingTint": "20" => 0.2f32, "50" => 0.5f32, |r| r.0.shading_tint;
        "ParagraphBorderOn": "false" => false, "true" => true, |r| r.0.border_on;
        "ParagraphBorderColor": "Color/Black" => "[Black]", "Swatch/None" => "[None]", |r| r.0.border_color;
        "ParagraphBorderTint": "-1" => 1.0f32, "50" => 0.5f32, |r| r.0.border_tint;
        "FontStyle": "Regular" => "Regular", "Bold" => "Bold", |r| r.1.font_style;
        "PointSize": "12" => 12.0, "9" => 9.0, |r| r.1.size;
        "Leading": "Auto" => Leading::Auto, "14" => Leading::Points(14.0), |r| r.1.leading;
        "KerningMethod": "$ID/Metrics" => designcraft_doc::Kerning::Metrics, "$ID/Optical" => designcraft_doc::Kerning::Optical, |r| r.1.kerning;
        "Tracking": "0" => 0.0, "20" => 20.0, |r| r.1.tracking;
        "HorizontalScale": "100" => 1.0, "90" => 0.9, |r| r.1.h_scale;
        "VerticalScale": "100" => 1.0, "90" => 0.9, |r| r.1.v_scale;
        "BaselineShift": "0" => 0.0, "2" => 2.0, |r| r.1.baseline_shift;
        "Skew": "0" => 0.0, "10" => 10.0, |r| r.1.skew;
        "FillColor": "Color/Black" => "[Black]", "Swatch/None" => "[None]", |r| r.1.fill;
        "FillTint": "-1" => 1.0, "50" => 0.5, |r| r.1.fill_tint;
        "StrokeColor": "Swatch/None" => "[None]", "Color/Black" => "[Black]", |r| r.1.stroke;
        "StrokeTint": "-1" => 1.0, "50" => 0.5, |r| r.1.stroke_tint;
        "StrokeWeight": "1" => 1.0, "2" => 2.0, |r| r.1.stroke_weight;
        "Capitalization": "Normal" => Capitalization::Normal, "AllCaps" => Capitalization::AllCaps, |r| r.1.capitalization;
        "Position": "Normal" => Position::Normal, "Superscript" => Position::Superscript, |r| r.1.position;
        "Underline": "false" => false, "true" => true, |r| r.1.underline;
        "StrikeThru": "false" => false, "true" => true, |r| r.1.strikethrough;
        "UnderlineTint": "-1" => 1.0f32, "50" => 0.5f32, |r| r.1.underline_tint;
        "StrikeThruTint": "-1" => 1.0f32, "50" => 0.5f32, |r| r.1.strikethrough_tint;
        "Ligatures": "true" => true, "false" => false, |r| r.1.ligatures;
        "NoBreak": "false" => false, "true" => true, |r| r.1.no_break;
        "OTFContextualAlternate": "true" => true, "false" => false, |r| designcraft_doc::otf::is_on(&r.1.otf_features, "calt");
        "DigitsType": "DefaultDigits" => Digits::Default, "ArabicDigits" => Digits::Arabic, |r| r.1.digits;
        "AppliedLanguage": "$ID/English: USA" => "English: USA", "$ID/German: 2006 Reform" => "German: 2006 Reform", |r| r.1.language;
    }
}

/// [No paragraph style] with `root` attributes and `Body`, based on it, with `body` attributes.
fn text_styles(root: &str, body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<idPkg:Styles xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <RootParagraphStyleGroup Self="rp">
    <ParagraphStyle Self="ParagraphStyle/$ID/[No paragraph style]" Name="$ID/[No paragraph style]" {root}/>
    <ParagraphStyle Self="ParagraphStyle/Body" Name="Body" {body}>
      <Properties><BasedOn type="object">ParagraphStyle/$ID/[No paragraph style]</BasedOn></Properties>
    </ParagraphStyle>
  </RootParagraphStyleGroup>
</idPkg:Styles>"#
    )
}

fn resolve_body(root: &str, body: &str) -> Resolved {
    let d = import_idml_with(&fixture_with_styles(&text_styles(root, body)), &|_| None).unwrap();
    d.styles.resolve_para_style("Body")
}

/// Text attributes an IDML file leaves unset are InDesign's defaults; written ones win, and a
/// file that writes every default (as InDesign's own exports do) imports the same.
#[test]
fn absent_text_attributes_take_indesign_defaults() {
    let rows = indesign_text_defaults();
    let all = |pick: fn(&AttrRow) -> &str| rows.iter().map(|r| format!(r#"{}="{}""#, r.0, pick(r))).collect::<Vec<_>>().join(" ");
    let (defaults, others) = (all(|r| r.1), all(|r| r.2));
    let omitted = resolve_body("", "");
    let written = resolve_body(&defaults, "");
    for (attr, _, _, field, want, _) in &rows {
        assert_eq!(field(&omitted), *want, "{attr} omitted");
        assert_eq!(field(&written), *want, "{attr} written as InDesign's default");
    }
    assert_eq!(omitted, written);
    for (root, body) in [(others.as_str(), ""), ("", others.as_str())] {
        let r = resolve_body(root, body);
        for (attr, _, _, field, _, want) in &rows {
            assert_eq!(field(&r), *want, "{attr} written on the {}", if root.is_empty() { "style" } else { "root" });
        }
    }
    // Product difference: InDesign's default face isn't available, so unset fonts are DesignCraft's.
    assert_eq!(omitted.1.font_family, designcraft_doc::CharProps::default().font_family);
}

/// A tint of -1 is the colour's own tint (100 %): written on a style, it replaces its parent's.
/// Tints are never negative or above 100 %.
#[test]
fn minus_one_tint_is_the_colours_own() {
    let tints = |v: &str| {
        ["UnderlineTint", "StrikeThruTint", "FillTint", "StrokeTint", "ParagraphShadingTint", "ParagraphBorderTint", "RuleAboveTint", "RuleBelowTint"]
            .map(|k| format!(r#"{k}="{v}""#))
            .join(" ")
            + r#" RuleAbove="true" RuleBelow="true""#
    };
    for (body, want) in [("-1", 1.0), ("150", 1.0)] {
        let r = resolve_body(&tints("50"), &tints(body));
        let got = [
            r.1.underline_tint,
            r.1.strikethrough_tint,
            r.1.fill_tint,
            r.1.stroke_tint,
            r.0.shading_tint,
            r.0.border_tint,
            r.0.rule_above.tint,
            r.0.rule_below.tint,
        ];
        assert_eq!(got, [want; 8], "{body}");
    }
    // Footnote rules, table fills and strokes, cell fills.
    let prefs = PREFS.replace("</idPkg:Preferences>", r#"<FootnoteOption RuleTint="-1"/></idPkg:Preferences>"#);
    let story = r#"<?xml version="1.0" encoding="UTF-8"?>
<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
<Story Self="s1"><ParagraphStyleRange><CharacterStyleRange><Table Self="tb" StartRowFillCount="1" StartRowFillTint="-1" EndRowFillTint="-1" TopBorderStrokeTint="-1">
<Row Self="r0" Name="0"/><Column Self="c0" Name="0"/><Cell Self="ce" Name="0:0" FillColor="Color/Black" FillTint="-1" TopEdgeStrokeTint="-1"/></Table></CharacterStyleRange></ParagraphStyleRange></Story>
</idPkg:Story>"#;
    let d = import_idml_with(
        &zip_files(&[
            ("designmap.xml", DESIGNMAP),
            ("Resources/Graphic.xml", GRAPHIC),
            ("Resources/Styles.xml", STYLES),
            ("Resources/Preferences.xml", &prefs),
            ("MasterSpreads/MasterSpread_m1.xml", MASTER),
            ("Spreads/Spread_sp1.xml", SPREAD),
            ("Stories/Story_s1.xml", story),
        ]),
        &|_| None,
    )
    .unwrap();
    assert_eq!(d.footnote_options.rule.tint, 1.0);
    let t = d.stories.values().flat_map(|s| s.tables.values()).next().unwrap();
    let alt = t.options.alt_rows.as_ref().unwrap();
    assert_eq!((alt.first_tint, alt.next_tint, t.options.border.tint), (1.0, 1.0, 1.0));
    let c = t.cell(0, 0).unwrap();
    assert_eq!((c.fill_tint, c.strokes[0].tint), (1.0, 1.0));
}

/// Rules in Text Color take the colour of the paragraph's text; they round-trip as Text Color.
#[test]
fn text_color_rules_round_trip() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let rule = designcraft_doc::Rule { on: true, ..Default::default() };
    assert_eq!(rule.color, designcraft_doc::TEXT_COLOR);
    let para = designcraft_doc::ParaAttrs {
        rule_above: Some(rule.clone()),
        rule_below: Some(designcraft_doc::Rule { color: "[Black]".into(), ..rule }),
        kashidas: Some(false),
        ..Default::default()
    };
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(40.0, 40.0, 400.0, 400.0), lid, "Ruled", ParaFormat::default()).unwrap();
    d.story_mut(sid).unwrap().paras[0].para = para.clone();
    let bytes = export_idml(&d);
    let story = zip_text(&bytes, "Stories/");
    assert!(story.contains(r#"<RuleAboveColor type="string">Text Color</RuleAboveColor>"#), "{story}");
    assert!(story.contains(r#"Kashidas="KashidasOff""#), "{story}");
    let back = import_idml(&bytes).unwrap();
    let p = &back.stories.values().find(|s| s.text == "Ruled").unwrap().paras[0];
    assert_eq!((p.para.rule_above.as_ref(), p.para.rule_below.as_ref()), (para.rule_above.as_ref(), para.rule_below.as_ref()));
    assert_eq!(p.para.kashidas, Some(false));
}

/// `[Basic Paragraph]` is InDesign's `$ID/NormalParagraphStyle`, both ways.
#[test]
fn basic_paragraph_is_normal_paragraph_style() {
    let d = import_idml_with(&fixture(), &|_| None).unwrap();
    let basic = d.styles.para(st::BASIC_PARAGRAPH).unwrap();
    assert_eq!(basic.based_on.as_deref(), Some(designcraft_doc::NO_PARA_STYLE));
    assert_eq!(d.styles.para("Text/Body").unwrap().based_on.as_deref(), Some(st::BASIC_PARAGRAPH));
    let s = d.stories.values().find(|s| s.text.starts_with("Hello")).unwrap();
    assert_eq!(s.paras[1].style, st::BASIC_PARAGRAPH);
    assert!(d.styles.paragraph.iter().all(|p| !p.name.contains("NormalParagraphStyle")));
    let bytes = export_idml(&d);
    let styles = zip_text(&bytes, "Resources/Styles.xml");
    assert!(styles.contains(r#"Self="ParagraphStyle/$ID/NormalParagraphStyle" Name="$ID/NormalParagraphStyle""#), "{styles}");
    assert!(zip_text(&bytes, "Stories/").contains(r#"AppliedParagraphStyle="ParagraphStyle/$ID/NormalParagraphStyle""#));
    let back = import_idml(&bytes).unwrap();
    assert_eq!(back.styles.paragraph.iter().filter(|p| p.name == st::BASIC_PARAGRAPH).count(), 1);
}

/// Page items, frames, tables and preferences an IDML file leaves unset are InDesign's defaults
/// for a new document.
#[test]
fn absent_item_and_document_attributes_take_indesign_defaults() {
    use designcraft_doc::{Cap, FirstBaseline, Join, StrokeAlign, VerticalJustification};
    use designcraft_geom::corners::CornerShape;
    let path = |x: f64| {
        let pts: String = [(x, 0.0), (x, 50.0), (x + 50.0, 50.0), (x + 50.0, 0.0)]
            .iter()
            .map(|(x, y)| format!(r#"<PathPointType Anchor="{x} {y}" LeftDirection="{x} {y}" RightDirection="{x} {y}"/>"#))
            .collect();
        format!(
            r#"<Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>{pts}</PathPointArray></GeometryPathType></PathGeometry></Properties>"#
        )
    };
    let designmap = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0" Self="d">
<NumberingList Self="NumberingList/Steps" Name="Steps"/>
<RootTableStyleGroup Self="rts"><TableStyle Self="TableStyle/Banded" Name="Banded" StartRowFillCount="1"/></RootTableStyleGroup>
<Story Self="s1"><ParagraphStyleRange><CharacterStyleRange><Table Self="tb" StartRowFillCount="1" StartColumnFillCount="2"><Row Self="r0" Name="0"/><Column Self="c0" Name="0"/><Cell Self="ce" Name="0:0"/></Table></CharacterStyleRange></ParagraphStyleRange></Story>
<Spread Self="sp1"><Page Self="p1"/>
<Rectangle Self="plain">{}</Rectangle>
<Rectangle Self="round" TopLeftCornerOption="RoundedCorner" TopRightCornerOption="RoundedCorner" BottomLeftCornerOption="RoundedCorner" BottomRightCornerOption="RoundedCorner">{}</Rectangle>
<Rectangle Self="legacy" CornerOption="RoundedCorner">{}</Rectangle>
<Rectangle Self="sized" TopLeftCornerOption="RoundedCorner" TopLeftCornerRadius="5">{}</Rectangle>
<TextFrame Self="tf" ParentStory="s1">{}</TextFrame>
</Spread>
</Document>"#,
        path(0.0),
        path(100.0),
        path(200.0),
        path(300.0),
        path(400.0)
    );
    let d = import_idml_with(&zip_files(&[("designmap.xml", &designmap)]), &|_| None).unwrap();
    // Document Setup, margins and columns, grids, Advanced Type, transparency, black.
    let s = &d.settings;
    assert_eq!((s.page_width, s.page_height, s.facing_pages, s.intent), (612.0, 792.0, true, designcraft_doc::Intent::Print));
    assert_eq!((s.bleed, s.slug), ([0.0; 4], [0.0; 4]));
    let page = &d.spreads[0].pages[0];
    assert_eq!((page.margins.top, page.margins.bottom, page.margins.inside, page.margins.outside), (36.0, 36.0, 36.0, 36.0));
    assert_eq!((page.columns.count, page.columns.gutter), (1, 12.0));
    assert_eq!((s.baseline_grid.start, s.baseline_grid.increment, s.baseline_grid.view_threshold), (36.0, 12.0, 0.75));
    assert_eq!((s.grid.horizontal, s.grid.vertical, s.grid.subdivisions, s.grid.in_back), (72.0, 72.0, 8, true));
    let a = &s.advanced_type;
    assert_eq!((a.superscript_size, a.superscript_position, a.subscript_size, a.subscript_position), (58.3, 33.3, 58.3, 33.3));
    assert_eq!((s.blend_space, s.overprint_black, s.keyboard_increment), (designcraft_doc::BlendSpace::Cmyk, true, 1.0));
    // Fill, stroke, transparency, wrap and corners of a frame with no attributes.
    let item = |n: usize| d.spreads[0].items[n].clone();
    let plain = item(0);
    assert!(plain.fill.is_none() && plain.stroke.is_none());
    let st = &plain.stroke;
    assert_eq!((st.weight, st.miter_limit, st.cap, st.join, st.align), (1.0, 4.0, Cap::Butt, Join::Miter, StrokeAlign::Center));
    assert_eq!((plain.opacity, plain.blend, plain.wrap.mode), (1.0, designcraft_color::BlendMode::Normal, designcraft_doc::WrapMode::None));
    assert!(plain.corners.is_none());
    // A corner shape without a size: 12 pt.
    for n in [1, 2] {
        assert_eq!(item(n).corners.corners.map(|c| (c.shape, c.size)), [(CornerShape::Rounded, 12.0); 4], "item {n}");
    }
    assert_eq!(item(3).corners.corners[0].size, 5.0);
    // Text Frame Options.
    let tf = item(4);
    let o = &tf.text_frame().unwrap().options;
    assert_eq!((o.columns, o.gutter, o.inset, o.balance_columns, o.ignore_wrap), (1, 12.0, [0.0; 4], false, false));
    assert_eq!((o.vertical_justification, o.first_baseline, o.first_baseline_min), (VerticalJustification::Top, FirstBaseline::Ascent, 0.0));
    assert_eq!((o.auto_size, o.auto_size_ref), (designcraft_doc::AutoSize::Off, 4), "auto-size from the centre");
    // Numbered lists don't continue across stories.
    assert_eq!(d.settings.lists, vec![designcraft_doc::NumberedList { name: "Steps".into(), continue_across_stories: false }]);
    // Table and cell options.
    let t = d.stories.values().flat_map(|s| s.tables.values()).next().unwrap();
    assert_eq!((t.options.space_before, t.options.space_after, t.options.repeat_header), (4.0, -4.0, true));
    assert_eq!((t.options.border.weight, t.options.border.color.as_str()), (1.0, "[Black]"));
    assert_eq!((t.rows[0].mode, t.rows[0].height), (designcraft_doc::RowHeightMode::AtLeast, 3.0));
    // Alternating fills: the first rows or columns Black 20 %, the next None.
    let fills = |a: &designcraft_doc::AltFills| (a.first_color.clone(), a.first_tint, a.next_color.clone(), a.next_tint, a.next);
    let want = ("[Black]".to_string(), 0.2f32, "[None]".to_string(), 1.0f32, 1);
    assert_eq!(fills(t.options.alt_rows.as_ref().unwrap()), want);
    let cols = t.options.alt_cols.as_ref().unwrap();
    assert_eq!((fills(cols), cols.first), (want.clone(), 2));
    let banded = d.styles.table.iter().find(|s| s.name == "Banded").unwrap();
    assert_eq!(fills(banded.alt_rows.as_ref().unwrap()), want);
    let c = t.cell(0, 0).unwrap();
    assert_eq!((c.insets, c.vj, c.fill.as_str()), ([4.0; 4], VerticalJustification::Top, "[None]"));
    for e in &c.strokes {
        assert_eq!((e.weight, e.color.as_str(), e.tint, &e.kind), (1.0, "[Black]", 1.0, &designcraft_doc::StrokeType::Solid));
    }
}

/// IDML has no attribute for a drop cap's character style: it is the nested style list's leading
/// "through 1 Dropcap" entry, read into the drop cap style and written back from it.
#[test]
fn drop_caps_import_and_round_trip() {
    // A style with a drop cap and its character style; a paragraph overriding both locally.
    let opener = r#"<ParagraphStyle Self="ParagraphStyle/Opener" Name="Opener" DropCapCharacters="1" DropCapLines="3">
        <Properties><AllNestedStyles type="list"><ListItem type="record">
          <AppliedCharacterStyle type="object">CharacterStyle/Strong</AppliedCharacterStyle><Delimiter type="enumeration">Dropcap</Delimiter>
          <Repetition type="long">1</Repetition><Inclusive type="boolean">true</Inclusive></ListItem></AllNestedStyles></Properties>
      </ParagraphStyle>
    </RootParagraphStyleGroup>"#;
    let styles = STYLES.replace("</RootParagraphStyleGroup>", opener);
    let story = r#"<?xml version="1.0" encoding="UTF-8"?>
<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <Story Self="s1">
    <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/Opener" DropCapCharacters="2" DropCapLines="2">
      <Properties><AllNestedStyles type="list"><ListItem type="record">
        <AppliedCharacterStyle type="object">CharacterStyle/$ID/[No character style]</AppliedCharacterStyle><Delimiter type="enumeration">Dropcap</Delimiter>
        <Repetition type="long">1</Repetition><Inclusive type="boolean">true</Inclusive></ListItem></AllNestedStyles></Properties>
      <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>Once upon a time</Content></CharacterStyleRange>
    </ParagraphStyleRange>
  </Story>
</idPkg:Story>"#;
    let bytes = zip_files(&[
        ("designmap.xml", DESIGNMAP),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Styles.xml", &styles),
        ("Resources/Preferences.xml", PREFS),
        ("MasterSpreads/MasterSpread_m1.xml", MASTER),
        ("Spreads/Spread_sp1.xml", SPREAD),
        ("Stories/Story_s1.xml", story),
    ]);
    let check = |d: &Document| {
        let st = d.styles.para("Opener").unwrap();
        assert_eq!((st.para.drop_cap_chars, st.para.drop_cap_lines), (Some(1), Some(3)));
        assert_eq!(st.para.drop_cap_style.as_deref(), Some("Strong"));
        assert_eq!(st.para.nested_styles, Some(vec![designcraft_doc::NestedStyle::drop_cap("Strong")]), "one entry, not duplicated");
        let p = &d.stories.values().find(|s| s.text.starts_with("Once")).unwrap().paras[0];
        assert_eq!((p.para.drop_cap_chars, p.para.drop_cap_lines), (Some(2), Some(2)));
        assert_eq!(p.para.drop_cap_style.as_deref(), Some(st::NO_CHAR_STYLE));
    };
    let d = import_idml_with(&bytes, &|_| None).unwrap();
    check(&d);
    let bytes = export_idml(&d);
    assert!(!zip_text(&bytes, "").contains("DropCapStyle"), "not an IDML attribute");
    check(&import_idml(&bytes).unwrap());
    // A drop cap style set without nested styles is written as one; Align Left Edge and Scale for
    // Descenders as DropcapDetail.
    let mut d = d;
    if let Some(s) = d.styles_mut().para_mut("Opener") {
        s.para.nested_styles = None;
        s.para.drop_cap_align_left = Some(false);
        s.para.drop_cap_scale_descenders = Some(true);
    }
    let bytes = export_idml(&d);
    assert!(!zip_text(&bytes, "").contains("DropCapStyle"));
    assert!(zip_text(&bytes, "Resources/Styles.xml").contains(r#"DropcapDetail="2""#));
    let back = import_idml(&bytes).unwrap();
    let st = back.styles.para("Opener").unwrap();
    assert_eq!((st.para.drop_cap_chars, st.para.drop_cap_lines, st.para.drop_cap_style.as_deref()), (Some(1), Some(3), Some("Strong")));
    assert_eq!((st.para.drop_cap_align_left, st.para.drop_cap_scale_descenders), (Some(false), Some(true)));
    assert_eq!(st.para.nested_styles, Some(vec![designcraft_doc::NestedStyle::drop_cap("Strong")]));
}

/// IDML styles and stories with an `Opener` style whose drop cap style is `Strong`, plus `styles`
/// (more paragraph styles) and `paras` (the story's paragraph ranges). `WORDS` in them stands for
/// a nested style list of `Strong` through 1 word.
fn drop_cap_fixture(styles: &str, paras: &str) -> Vec<u8> {
    let nested = |style: &str, delim: &str| {
        format!(
            r#"<AllNestedStyles type="list"><ListItem type="record"><AppliedCharacterStyle type="object">CharacterStyle/{style}</AppliedCharacterStyle><Delimiter type="enumeration">{delim}</Delimiter><Repetition type="long">1</Repetition><Inclusive type="boolean">true</Inclusive></ListItem></AllNestedStyles>"#
        )
    };
    let words = nested("Strong", "AnyWord");
    let opener = format!(
        r#"<ParagraphStyle Self="ParagraphStyle/Opener" Name="Opener" DropCapCharacters="1" DropCapLines="3"><Properties>{}</Properties></ParagraphStyle>
    {}
    </RootParagraphStyleGroup>"#,
        nested("Strong", "Dropcap"),
        styles.replace("WORDS", &words)
    );
    let styles = STYLES.replace("</RootParagraphStyleGroup>", &opener);
    let story = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <Story Self="s1">{}</Story>
</idPkg:Story>"#,
        paras.replace("WORDS", &words)
    );
    zip_files(&[
        ("designmap.xml", DESIGNMAP),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Styles.xml", &styles),
        ("Resources/Preferences.xml", PREFS),
        ("MasterSpreads/MasterSpread_m1.xml", MASTER),
        ("Spreads/Spread_sp1.xml", SPREAD),
        ("Stories/Story_s1.xml", &story),
    ])
}

/// A nested style list replaces the inherited one, drop cap style entry included: a style or
/// paragraph with a list of its own and no Dropcap entry has no drop cap style.
#[test]
fn own_nested_styles_clear_an_inherited_drop_cap_style() {
    let styles = r#"<ParagraphStyle Self="ParagraphStyle/Child" Name="Child"><Properties><BasedOn type="object">ParagraphStyle/Opener</BasedOn>WORDS</Properties></ParagraphStyle>
    <ParagraphStyle Self="ParagraphStyle/Plain" Name="Plain"><Properties>WORDS</Properties></ParagraphStyle>"#;
    let paras = r#"<ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/Opener"><Properties>WORDS</Properties>
      <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>Cleared</Content><Br/></CharacterStyleRange>
    </ParagraphStyleRange>
    <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/Opener">
      <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>Inherited</Content></CharacterStyleRange>
    </ParagraphStyleRange>"#;
    let d = import_idml_with(&drop_cap_fixture(styles, paras), &|_| None).unwrap();
    let cap = |name: &str| d.styles.para(name).unwrap().para.drop_cap_style.clone();
    assert_eq!(cap("Opener").as_deref(), Some("Strong"));
    assert_eq!(cap("Child").as_deref(), Some(st::NO_CHAR_STYLE), "its own list replaces Opener's");
    assert_eq!(cap("Plain"), None, "nothing to clear");
    let story = d.stories.values().find(|s| s.text.starts_with("Cleared")).unwrap();
    assert_eq!(story.paras[0].para.drop_cap_style.as_deref(), Some(st::NO_CHAR_STYLE));
    assert_eq!(story.paras[1].para.drop_cap_style, None);
    assert_eq!(d.styles.resolve_para(&story.paras[1]).0.drop_cap_style, "Strong");
}

/// Paragraphs in table cells and footnotes are cleared like story paragraphs.
#[test]
fn own_nested_styles_clear_an_inherited_drop_cap_style_in_cells_and_notes() {
    let opener = |text: &str| {
        format!(
            r#"<ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/Opener"><Properties>WORDS</Properties><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>{text}</Content></CharacterStyleRange></ParagraphStyleRange>"#
        )
    };
    let paras = format!(
        r#"<ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/NormalParagraphStyle"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]">
<Content>Body</Content><Footnote>{}</Footnote>
<Table HeaderRowCount="0" FooterRowCount="0"><Row Name="0" MinimumHeight="24"/><Column Name="0" SingleColumnWidth="60"/><Cell Name="0:0">{}</Cell></Table>
</CharacterStyleRange></ParagraphStyleRange>"#,
        opener("Note"),
        opener("Cell")
    );
    let d = import_idml_with(&drop_cap_fixture("", &paras), &|_| None).unwrap();
    let story = d.stories.values().find(|s| !s.notes.is_empty()).expect("story with a footnote");
    let note = &story.notes[0].text;
    assert_eq!(note.paras[0].para.drop_cap_style.as_deref(), Some(st::NO_CHAR_STYLE), "{:?}", note.text);
    let cell = &story.tables.values().next().expect("a table").cell(0, 0).unwrap().text;
    assert_eq!(cell.paras[0].para.drop_cap_style.as_deref(), Some(st::NO_CHAR_STYLE), "{:?}", cell.text);
}

/// A paragraph that clears its style's drop cap style without nested styles of its own round-trips:
/// it is written with the style's list, the Dropcap entry set to [None].
#[test]
fn a_cleared_drop_cap_style_round_trips() {
    let paras = r#"<ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/Opener">
      <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>Once upon a time</Content></CharacterStyleRange>
    </ParagraphStyleRange>"#;
    let mut d = import_idml_with(&drop_cap_fixture("", paras), &|_| None).unwrap();
    let sid = *d.stories.keys().next().unwrap();
    let clear = |d: &mut Document| d.story_mut(sid).unwrap().paras[0].para.drop_cap_style = Some(st::NO_CHAR_STYLE.into());
    let resolved = |d: &Document| {
        let st = d.stories.values().find(|s| s.text.starts_with("Once")).unwrap();
        d.styles.resolve_para(&st.paras[0]).0
    };
    clear(&mut d);
    let back = import_idml(&export_idml(&d)).unwrap();
    let pp = resolved(&back);
    assert_eq!(pp.drop_cap_style, st::NO_CHAR_STYLE);
    assert_eq!(pp.nested_styles, vec![designcraft_doc::NestedStyle::drop_cap(st::NO_CHAR_STYLE)]);
    // The style's drop cap style without nested styles: the paragraph is written with an empty
    // list, which clears it on import.
    if let Some(s) = d.styles_mut().para_mut("Opener") {
        s.para.nested_styles = None;
    }
    assert_eq!(resolved(&d).drop_cap_style, st::NO_CHAR_STYLE);
    let back = import_idml(&export_idml(&d)).unwrap();
    assert_eq!(back.styles.para("Opener").unwrap().para.drop_cap_style.as_deref(), Some("Strong"));
    let pp = resolved(&back);
    assert_eq!(pp.drop_cap_style, st::NO_CHAR_STYLE);
    assert!(pp.nested_styles.iter().all(|ns| ns.style != "Strong"), "{:?}", pp.nested_styles);
}

/// Frames take the corners and Text Frame Options they don't write from their object style chain,
/// then `[None]`. Per-corner attributes win over the legacy all-corners pair, at each level; a
/// corner shape without a radius takes the chain's radius.
#[test]
fn frames_inherit_object_style_corners_and_text_frame_options() {
    use designcraft_doc::{AutoSize, FirstBaseline, VerticalJustification};
    use designcraft_geom::corners::CornerShape as C;
    // Anchors in the order top-left, bottom-left, bottom-right, top-right.
    let path = |x: f64| {
        let pts: String = [(x, 0.0), (x, 50.0), (x + 50.0, 50.0), (x + 50.0, 0.0)]
            .iter()
            .map(|(x, y)| format!(r#"<PathPointType Anchor="{x} {y}" LeftDirection="{x} {y}" RightDirection="{x} {y}"/>"#))
            .collect();
        format!(
            r#"<Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>{pts}</PathPointArray></GeometryPathType></PathGeometry></Properties>"#
        )
    };
    let corners = |opt: &str, r: &str| {
        ["TopLeft", "TopRight", "BottomLeft", "BottomRight"].map(|n| format!(r#"{n}CornerOption="{opt}" {n}CornerRadius="{r}""#)).join(" ")
    };
    let inset = |v: f64| {
        format!(
            r#"<Properties><InsetSpacing type="list">{}</InsetSpacing></Properties>"#,
            r#"<ListItem type="unit">V</ListItem>"#.replace('V', &v.to_string()).repeat(4)
        )
    };
    let designmap = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="20.2" Self="d">
<RootObjectStyleGroup Self="ro">
  <ObjectStyle Self="ObjectStyle/$ID/[None]" Name="$ID/[None]" {none}>
    <TextFramePreference TextColumnCount="1" TextColumnGutter="12" AutoSizingReferencePoint="CenterPoint" AutoSizingType="Off"/>
  </ObjectStyle>
  <ObjectStyle Self="ObjectStyle/Rounded" Name="Rounded" TopLeftCornerOption="RoundedCorner" TopRightCornerOption="RoundedCorner" BottomLeftCornerOption="RoundedCorner" BottomRightCornerOption="RoundedCorner" TopLeftCornerRadius="6">
    <Properties><BasedOn type="object">ObjectStyle/$ID/[None]</BasedOn></Properties>
    <TextFramePreference TextColumnCount="2" TextColumnGutter="18" VerticalJustification="CenterAlign" FirstBaselineOffset="LeadingOffset" MinimumFirstBaselineOffset="4" IgnoreWrap="true" AutoSizingType="HeightOnly">{inset}</TextFramePreference>
  </ObjectStyle>
  <ObjectStyle Self="ObjectStyle/Child" Name="Child" TopRightCornerOption="BevelCorner">
    <Properties><BasedOn type="object">ObjectStyle/Rounded</BasedOn></Properties>
  </ObjectStyle>
</RootObjectStyleGroup>
<Story Self="s1"/>
<Spread Self="sp1"><Page Self="p1"/>
<TextFrame Self="tf" ParentStory="s1" AppliedObjectStyle="ObjectStyle/Child" BottomRightCornerOption="InverseRoundedCorner" BottomRightCornerRadius="11.34">{p0}<TextFramePreference TextColumnCount="3"/></TextFrame>
<Rectangle Self="legacy" AppliedObjectStyle="ObjectStyle/Rounded" CornerOption="BevelCorner" CornerRadius="5" TopLeftCornerOption="InsetCorner">{p1}</Rectangle>
<Rectangle Self="one" TopRightCornerOption="RoundedCorner" TopRightCornerRadius="11.34">{p2}</Rectangle>
<Rectangle Self="plain" AppliedObjectStyle="ObjectStyle/Rounded">{p3}</Rectangle>
</Spread>
</Document>"#,
        none = corners("None", "12"),
        inset = inset(3.0),
        p0 = path(0.0),
        p1 = path(100.0),
        p2 = path(200.0),
        p3 = path(300.0),
    );
    let d = import_idml_with(&zip_files(&[("designmap.xml", &designmap)]), &|_| None).unwrap();
    let item = |n: usize| d.spreads[0].items[n].clone();
    // Path order: top-left, bottom-left, bottom-right, top-right.
    let shapes = |n: usize| item(n).corners.corners.map(|c| (c.shape, c.size));
    assert_eq!(shapes(0), [(C::Rounded, 6.0), (C::Rounded, 12.0), (C::InverseRounded, 11.34), (C::Bevel, 12.0)]);
    assert_eq!(shapes(1), [(C::Inset, 5.0), (C::Bevel, 5.0), (C::Bevel, 5.0), (C::Bevel, 5.0)]);
    assert!(item(2).corners.corners.iter().enumerate().all(|(i, c)| (c.shape == C::Rounded) == (i == 3)));
    assert_eq!(item(2).corners.corners[3].size, 11.34);
    assert_eq!(shapes(3), [(C::Rounded, 6.0), (C::Rounded, 12.0), (C::Rounded, 12.0), (C::Rounded, 12.0)]);
    let tf = item(0);
    let o = &tf.text_frame().unwrap().options;
    assert_eq!((o.columns, o.gutter, o.inset), (3, 18.0, [3.0; 4]));
    assert_eq!((o.vertical_justification, o.first_baseline, o.first_baseline_min), (VerticalJustification::Center, FirstBaseline::Leading, 4.0));
    assert_eq!((o.ignore_wrap, o.auto_size, o.auto_size_ref), (true, AutoSize::HeightOnly, 4));
}
