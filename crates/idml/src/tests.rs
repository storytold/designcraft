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
    assert_eq!(story.text, format!("Hello & bold\nPage {}\tend{}after", st::PAGE_NUMBER, st::FRAME_BREAK));
    assert_eq!(story.paras.len(), 2);
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
        s.insert(end, &format!(" {}", st::COLUMN_BREAK));
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
