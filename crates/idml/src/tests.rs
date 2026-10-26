use std::io::Write;

use designcraft_color::{Color, Swatch, SwatchValue};
use designcraft_doc::build::NewDocument;
use designcraft_doc::{CharAttrs, Document, Fill, ParaFormat, Shape, SpreadRef, story as st};
use designcraft_geom::{Rect, shapes};

use super::*;

mod assets;
mod clipping;
mod decorations;
mod facing_ltr;
mod frame_grid;
mod index_sort;
mod liquid_guides;
mod object_styles;
mod rtl_binding;
mod spread_rotation;
mod variables;
mod wrap;

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
        "IgnoreEdgeAlignment": "false" => false, "true" => true, |r| r.0.ignore_optical_margin;
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

fn optical_story(pref: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <Story Self="s1">
    <StoryPreference {pref} StoryOrientation="Horizontal" StoryDirection="LeftToRightDirection"/>
    <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/Body">
      <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>“Hung” here.</Content><Br/></CharacterStyleRange>
    </ParagraphStyleRange>
    <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/Body" IgnoreEdgeAlignment="true">
      <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>“Flush” here.</Content></CharacterStyleRange>
    </ParagraphStyleRange>
  </Story>
</idPkg:Story>"#
    )
}

/// Story ▸ Optical Margin Alignment and its size, and Ignore Optical Margin on paragraphs and
/// paragraph styles, survive import and export.
#[test]
fn story_optical_margin_round_trips() {
    let story = |d: &Document| d.stories.values().find(|s| s.text.starts_with("“Hung”")).cloned().unwrap();
    let d = import_idml_with(&fixture_with_story(&optical_story(r#"OpticalMarginAlignment="true" OpticalMarginSize="18""#)), &|_| None).unwrap();
    let s = story(&d);
    assert_eq!(s.optical_margin, designcraft_doc::OpticalMargin { on: true, size: 18.0 });
    assert_eq!((s.paras[0].para.ignore_optical_margin, s.paras[1].para.ignore_optical_margin), (None, Some(true)));
    let bytes = export_idml(&d);
    let text = zip_text(&bytes, "Stories/");
    assert!(text.contains(r#"OpticalMarginAlignment="true" OpticalMarginSize="18""#), "{text}");
    assert!(text.contains(r#"IgnoreEdgeAlignment="true""#), "{text}");
    let back = story(&import_idml(&bytes).unwrap());
    assert_eq!(back.optical_margin, s.optical_margin);
    assert_eq!(back.paras[1].para.ignore_optical_margin, Some(true));
    // Sizes outside InDesign's range are clamped; malformed ones and an absent preference are the default.
    for (pref, want) in [
        (r#"OpticalMarginAlignment="true" OpticalMarginSize="1e9""#, (true, 1296.0)),
        (r#"OpticalMarginAlignment="true" OpticalMarginSize="0""#, (true, 0.1)),
        (r#"OpticalMarginAlignment="true" OpticalMarginSize="NaN""#, (true, 12.0)),
        (r#"OpticalMarginAlignment="false""#, (false, 12.0)),
    ] {
        let d = import_idml_with(&fixture_with_story(&optical_story(pref)), &|_| None).unwrap();
        let om = story(&d).optical_margin;
        assert_eq!((om.on, om.size), want, "{pref}");
    }
    // On a paragraph style.
    let d = import_idml_with(&fixture_with_styles(&keep_styles(r#"IgnoreEdgeAlignment="true""#)), &|_| None).unwrap();
    assert!(d.styles.resolve_para_style("Body").0.ignore_optical_margin);
    let styles = zip_text(&export_idml(&d), "Resources/Styles.xml");
    assert!(styles.contains(r#"IgnoreEdgeAlignment="true""#), "{styles}");
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
    let mut bt = designcraft_doc::Table::new(0, 1, 1, 0, 0, 100.0);
    d.styles.resolve_table_style(&banded.name).apply_to(&mut bt, &d.styles.cell);
    assert_eq!(fills(bt.options.alt_rows.as_ref().unwrap()), want);
    let c = t.cell(0, 0).unwrap();
    assert_eq!((c.insets, c.vj, c.fill.as_str()), ([4.0; 4], VerticalJustification::Top, "[None]"));
    for e in &c.strokes {
        assert_eq!((e.weight, e.color.as_str(), e.tint, &e.kind), (1.0, "[Black]", 1.0, &designcraft_doc::StrokeType::Solid));
    }
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

/// A section numbered in Kanji: page names, the folio a page-number marker draws, and a round trip.
#[test]
fn kanji_page_numbers() {
    let designmap = DESIGNMAP.replace(">LowerRoman<", ">Kanji<");
    let bytes = zip_files(&[
        ("designmap.xml", &designmap),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Styles.xml", STYLES),
        ("Resources/Preferences.xml", PREFS),
        ("MasterSpreads/MasterSpread_m1.xml", MASTER),
        ("Spreads/Spread_sp1.xml", SPREAD),
        ("Stories/Story_s1.xml", STORY),
    ]);
    let mut d = import_idml(&bytes).unwrap();
    assert_eq!(d.sections[0].style, designcraft_doc::NumberStyle::Kanji);
    assert_eq!((d.page_name(0), d.page_name(1)), ("五".to_string(), "六".to_string()));
    let back = import_idml(&export_idml(&d)).unwrap();
    assert_eq!(back.sections[0].style, designcraft_doc::NumberStyle::Kanji);
    assert_eq!(back.page_name(1), "六");

    // The marker draws the same glyphs as the page name typed in its place.
    let sid = *d.stories.keys().next().unwrap();
    let at = d.story(sid).unwrap().text.find(st::PAGE_NUMBER).unwrap();
    let marker = |d: &Document| {
        let cs = designcraft_compose::compose_story(d, sid, &Default::default());
        cs.frames
            .iter()
            .enumerate()
            .find_map(|(fi, f)| {
                let gids: Vec<u32> = f.lines.iter().flat_map(|l| &l.glyphs).filter(|g| g.byte == at).map(|g| g.gid).collect();
                (!gids.is_empty()).then_some((fi, gids))
            })
            .unwrap()
    };
    let (frame, folio) = marker(&d);
    let name = d.page_name(frame);
    assert_eq!(name.len(), st::PAGE_NUMBER.len_utf8(), "same byte length keeps the story's ranges");
    let story = d.story_mut(sid).unwrap();
    story.text.replace_range(at..at + name.len(), &name);
    assert_eq!(marker(&d), (frame, folio));
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
    assert_eq!(gradient.stops[0].swatch.as_deref(), Some("Brand"), "a stop on a swatch keeps it");
    assert_eq!(gradient.stops[1].swatch, None, "an unnamed color stays plain");
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

/// A one-page spread part `Self` with one rectangle.
fn one_frame_spread(id: &str) -> String {
    format!(
        r#"<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
          <Spread Self="{id}">
            <Page Self="{id}p" GeometricBounds="0 0 100 100" ItemTransform="1 0 0 1 0 0"/>
            <Rectangle Self="{id}r" ItemTransform="1 0 0 1 0 0">
              <Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
                <PathPointType Anchor="10 10"/><PathPointType Anchor="10 50"/>
                <PathPointType Anchor="50 50"/><PathPointType Anchor="50 10"/>
              </PathPointArray></GeometryPathType></PathGeometry></Properties>
            </Rectangle>
          </Spread>
        </idPkg:Spread>"#
    )
}

/// Writers differ in how a designmap `src` spells its part: percent-encoded, another case,
/// backslashes, a leading `./`. Those resolve; a part absent from the package is a warning that
/// names it, not silently dropped objects.
#[test]
fn designmap_parts_match_leniently_and_missing_ones_are_reported() {
    let designmap = r#"<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" Self="d">
          <idPkg:Spread src="Spreads/Spread_u1a%20b.xml"/>
          <idPkg:Spread src="./spreads\SPREAD_u2.xml"/>
          <idPkg:Spread src="Spreads/Spread_gone.xml"/>
        </Document>"#;
    let zip = zip_files(&[
        ("designmap.xml", designmap),
        ("Spreads/Spread_u1a b.xml", &one_frame_spread("u1")),
        ("Spreads/Spread_u2.xml", &one_frame_spread("u2")),
    ]);
    let r = import_idml_report(&zip, &|_| None).unwrap();
    assert_eq!(r.document.spreads.len(), 2, "both lenient spellings resolve");
    assert!(r.document.spreads.iter().all(|sp| sp.items.len() == 1), "the spreads keep their frames");
    assert_eq!(r.warnings.len(), 1, "{:?}", r.warnings);
    assert!(r.warnings[0].contains("Spreads/Spread_gone.xml"), "{:?}", r.warnings);

    // With every spread part missing the import fails, and the error names the part.
    let only_missing = r#"<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" Self="d">
          <idPkg:Spread src="Spreads/Spread_gone.xml"/>
        </Document>"#;
    let err = import_idml(&zip_files(&[("designmap.xml", only_missing)])).unwrap_err();
    assert!(err.to_string().contains("Spreads/Spread_gone.xml"), "{err}");
}

#[test]
fn percent_decode_keeps_malformed_escapes() {
    assert_eq!(import::percent_decode("a%20b%2"), "a b%2");
    assert_eq!(import::percent_decode("%é%zz"), "%é%zz");
    assert_eq!(import::uri_to_path("file:///C:/a%20b"), "C:/a b");
    assert_eq!(import::uri_to_path("file:/a%bé"), "/a%bé");
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
fn gradient_stops_keep_their_swatches_through_a_round_trip() {
    use designcraft_color::{ColorType, Gradient, GradientKind, GradientStop};
    let mut d = Document::new(&NewDocument::default());
    let gold = Color::cmyk(0.0, 0.3, 0.9, 0.1);
    d.swatches.push(Swatch { value: SwatchValue::Color { color: gold, color_type: ColorType::Spot }, ..Swatch::color("Gold", gold) });
    d.swatches.push(Swatch { value: SwatchValue::Tint { base: "Gold".into(), tint: 0.4 }, ..Swatch::color("Gold 40%", gold) });
    let on = |offset: f32, name: &str| GradientStop {
        swatch: Some(name.into()),
        ..GradientStop::new(offset, designcraft_color::swatch::resolve(&d.swatches, name, 1.0).unwrap())
    };
    let stops = vec![on(0.0, "Gold"), on(0.3, "Gold 40%"), GradientStop::new(0.6, Color::cmyk(0.5, 0.0, 0.0, 0.0)), on(1.0, "[Paper]")];
    d.swatches.push(Swatch {
        value: SwatchValue::Gradient { gradient: Gradient { kind: GradientKind::Linear, stops } },
        ..Swatch::color("Gold fade", gold)
    });
    let back = import_idml(&export_idml(&d)).unwrap();
    let SwatchValue::Gradient { gradient } = &back.swatch("Gold fade").unwrap().value else { panic!("gradient") };
    let names: Vec<Option<&str>> = gradient.stops.iter().map(|s| s.swatch.as_deref()).collect();
    assert_eq!(names, [Some("Gold"), Some("Gold 40%"), None, Some("[Paper]")]);
    assert_eq!(gradient.stops[2].color, Color::cmyk(0.5, 0.0, 0.0, 0.0));
}

#[test]
fn round_trips_overridden_parent_items() {
    let mut d = Document::new(&NewDocument { pages: 1, ..Default::default() });
    let lid = d.default_layer();
    let (ppi, ppage) = d.parent_page_for(0).unwrap();
    let px = d.parents[ppi].pages[ppage].x;
    let pid = designcraft_doc::ItemId(d.alloc());
    let frame = designcraft_doc::Item::new(pid, lid, Shape::Rectangle, shapes::rectangle(Rect::new(px + 40.0, 40.0, px + 200.0, 120.0)));
    d.insert_item(SpreadRef::Parent(ppi), frame.clone(), None).unwrap();
    // What layout.overrideParentItems leaves behind: a linked local copy, and the parent item listed on the page.
    let mut copy = frame;
    copy.id = designcraft_doc::ItemId(d.alloc());
    copy.xf = designcraft_geom::Affine::translate((d.spreads[0].pages[0].x - px, 0.0)) * copy.xf;
    copy.overrides = Some(pid);
    d.insert_item(SpreadRef::Doc(0), copy, None).unwrap();
    std::sync::Arc::make_mut(&mut d.spreads[0]).pages[0].overridden.push(pid);

    let back = import_idml(&export_idml(&d)).unwrap();
    let parent_ids: Vec<_> = back.parents[ppi].items.iter().map(|i| i.id).collect();
    assert_eq!(parent_ids.len(), 1);
    let page = &back.spreads[0].pages[0];
    assert_eq!(page.overridden, parent_ids);
    let local: Vec<_> = back.spreads[0].items.iter().collect();
    assert_eq!(local.len(), 1);
    assert_eq!(local[0].overrides, Some(parent_ids[0]));
    // Rendering skips overridden parent items, so page 1 shows only the local copy.
    let shown_parent = back.parents[ppi].items.iter().filter(|i| !page.overridden.contains(&i.id)).count();
    assert_eq!(shown_parent + local.len(), 1);
}

#[test]
fn uri_and_base64_helpers() {
    assert_eq!(import::uri_to_path("file:/a%20b/c.png"), "/a b/c.png");
    assert_eq!(import::uri_to_path("file:///C:/x/y.jpg"), "C:/x/y.jpg");
    assert_eq!(import::uri_to_path("file://localhost/a/b.png"), "/a/b.png");
    assert_eq!(import::uri_to_path("file://C:/x/y.jpg"), "C:/x/y.jpg");
    // Another computer stays one: a network path, not a folder named after it.
    assert_eq!(import::uri_to_path("file://192.0.2.1/s/x%20y.png"), "//192.0.2.1/s/x y.png");
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
fn explicitly_hidden_cell_edges_hide_the_default_table_border() {
    // No table border or table style is given, and every cell edge is explicitly hidden (issue #1623):
    // the first cell by weight 0, the second by the None swatch.
    let cell = |name: &str, attr: &str, value: &str| {
        format!(
            r#"<Cell Name="{name}" LeftEdge{attr}="{value}" RightEdge{attr}="{value}" TopEdge{attr}="{value}" BottomEdge{attr}="{value}">
    <ParagraphStyleRange><CharacterStyleRange><Content>X</Content></CharacterStyleRange></ParagraphStyleRange></Cell>"#
        )
    };
    let story = format!(
        r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
<Story Self="s1"><ParagraphStyleRange><CharacterStyleRange>
<Table HeaderRowCount="0" FooterRowCount="0">
  <Row Name="0" MinimumHeight="24"/>
  <Column Name="0" SingleColumnWidth="60"/><Column Name="1" SingleColumnWidth="60"/>
  {}{}
</Table></CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>"#,
        cell("0:0", "StrokeWeight", "0"),
        cell("1:0", "StrokeColor", "Swatch/None")
    );
    let d = import_idml(&fixture_with_story(&story)).unwrap();
    d.check().unwrap();
    let table = d.stories.values().flat_map(|s| s.tables.values()).next().unwrap();
    for c in 0..2 {
        assert_eq!(table.cell(0, c).unwrap().border_overrides, [true; 4], "cell {c} suppresses the table border");
    }
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

/// A cross-reference source sits either inside a character range (holding the text directly) or at
/// paragraph level around a range. Both give the generated text the paragraph's and the range's
/// attributes, so both set the same text at the same width.
#[test]
fn cross_reference_source_nesting_keeps_formatting() {
    let story = |body: &str| {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
<Story Self="s1"><ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/NormalParagraphStyle" PointSize="11">{body}</ParagraphStyleRange></Story></idPkg:Story>"#
        )
    };
    const CSR: &str = r#"<CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" Tracking="200">"#;
    let inside = story(&format!(
        r#"{CSR}<Content>See section </Content><CrossReferenceSource Self="x1" AppliedFormat="n"><Content>11.</Content></CrossReferenceSource><Content>, then a few more words to wrap.</Content></CharacterStyleRange>"#
    ));
    let around = story(&format!(
        r#"{CSR}<Content>See section </Content></CharacterStyleRange><CrossReferenceSource Self="x1" AppliedFormat="n">{CSR}<Content>11.</Content></CharacterStyleRange></CrossReferenceSource>{CSR}<Content>, then a few more words to wrap.</Content></CharacterStyleRange>"#
    ));
    let compose = |xml: &str| {
        let d = import_idml(&fixture_with_story(xml)).unwrap();
        d.check().unwrap();
        let s = d.stories.values().find(|s| !s.xrefs.is_empty()).expect("story with a cross-reference").clone();
        let cs = designcraft_compose::compose_story(&d, s.id, &Default::default());
        let glyphs: Vec<_> = cs.frames.iter().flat_map(|f| &f.lines).flat_map(|l| l.glyphs.iter().map(move |g| (g.byte, g.x, l.baseline))).collect();
        (s, glyphs)
    };
    let (a, ga) = compose(&inside);
    let (b, gb) = compose(&around);
    assert!(ga.len() > a.text.chars().count() / 2, "the story composes");
    assert_eq!(a.text, b.text);
    assert_eq!(a.chars, b.chars);
    assert_eq!(a.chars.len(), 1, "the generated text shares its neighbours' format");
    assert_eq!(a.chars[0].format.over.tracking, Some(200.0));
    assert_eq!(a.chars[0].format.over.size, Some(11.0));
    assert_eq!(ga, gb);
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

/// Anchored settings an item leaves unset come from its object style chain, attribute by attribute.
#[test]
fn anchored_objects_take_unset_settings_from_object_styles() {
    use designcraft_doc::{AnchorPosition, anchored::AnchorRelative as R};
    let styles = STYLES.replace(
        r#"<ObjectStyle Self="ObjectStyle/$ID/[None]" Name="$ID/[None]"/>"#,
        r#"<ObjectStyle Self="ObjectStyle/$ID/[None]" Name="$ID/[None]">
          <AnchoredObjectSetting AnchoredPosition="InlinePosition" SpineRelative="false" LockPosition="false" PinPosition="true"
            AnchorPoint="BottomRightAnchor" HorizontalAlignment="LeftAlign" HorizontalReferencePoint="TextFrame"
            VerticalAlignment="BottomAlign" VerticalReferencePoint="LineBaseline" AnchorXoffset="0" AnchorYoffset="0" AnchorSpaceAbove="0"/>
        </ObjectStyle>
        <ObjectStyle Self="ObjectStyle/Side" Name="Side"><AnchoredObjectSetting HorizontalReferencePoint="PageMargins" PinPosition="false"/></ObjectStyle>
        <ObjectStyle Self="ObjectStyle/Note" Name="Note"><Properties><BasedOn type="object">ObjectStyle/Side</BasedOn></Properties></ObjectStyle>"#,
    );
    let rect = |style: &str, setting: &str| {
        format!(
            r#"<Rectangle Self="r{style}" AppliedObjectStyle="ObjectStyle/{style}" ItemTransform="1 0 0 1 0 0">
              <Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
                <PathPointType Anchor="0 0"/><PathPointType Anchor="0 10"/><PathPointType Anchor="10 10"/><PathPointType Anchor="10 0"/>
              </PathPointArray></GeometryPathType></PathGeometry></Properties>
              <AnchoredObjectSetting AnchoredPosition="Anchored" {setting}/>
            </Rectangle>"#
        )
    };
    let story = format!(
        r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Story Self="s1">
          <ParagraphStyleRange><CharacterStyleRange><Content>a</Content>{}{}{}</CharacterStyleRange></ParagraphStyleRange>
        </Story></idPkg:Story>"#,
        rect("$ID/[None]", r#"AnchorXoffset="5" AnchorYoffset="-3""#),
        rect("$ID/[None]", r#"AnchorPoint="TopLeftAnchor" VerticalReferencePoint="PageEdge""#),
        rect("Note", r#"AnchorXoffset="2""#),
    );
    let d = import_idml(&fixture_with_table_styles(&styles, &story)).unwrap();
    let s = d.stories.values().find(|s| !s.objects.is_empty()).unwrap();
    let custom = |x_relative, y_relative, x_offset, y_offset, object_point, keep_within_column| AnchorPosition::Custom {
        x_relative,
        y_relative,
        x_offset,
        y_offset,
        object_point,
        ref_point: 6,
        keep_within_column,
        spine_relative: false,
    };
    let positions: Vec<_> = s.objects.iter().map(|o| o.position.clone()).collect();
    assert_eq!(
        positions,
        [
            custom(R::TextFrame, R::Anchor, -5.0, -3.0, 8, true),
            custom(R::TextFrame, R::PageEdge, 0.0, 0.0, 0, true),
            custom(R::PageMargin, R::Anchor, -2.0, 0.0, 8, false),
        ]
    );
}

/// Relative to Spine comes from the object style chain like the other anchored settings, an
/// item's own setting wins, and export writes it back.
#[test]
fn anchored_spine_relative_comes_from_object_styles_and_round_trips() {
    use designcraft_doc::AnchorPosition;
    let styles = STYLES.replace(
        r#"<ObjectStyle Self="ObjectStyle/$ID/[None]" Name="$ID/[None]"/>"#,
        r#"<ObjectStyle Self="ObjectStyle/$ID/[None]" Name="$ID/[None]"/>
        <ObjectStyle Self="ObjectStyle/Side" Name="Side"><AnchoredObjectSetting SpineRelative="true"/></ObjectStyle>
        <ObjectStyle Self="ObjectStyle/Note" Name="Note"><Properties><BasedOn type="object">ObjectStyle/Side</BasedOn></Properties></ObjectStyle>"#,
    );
    let rect = |style: &str, setting: &str| {
        format!(
            r#"<Rectangle Self="r{style}" AppliedObjectStyle="ObjectStyle/{style}" ItemTransform="1 0 0 1 0 0">
              <Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
                <PathPointType Anchor="0 0"/><PathPointType Anchor="0 10"/><PathPointType Anchor="10 10"/><PathPointType Anchor="10 0"/>
              </PathPointArray></GeometryPathType></PathGeometry></Properties>
              <AnchoredObjectSetting AnchoredPosition="Anchored" {setting}/>
            </Rectangle>"#
        )
    };
    let story = format!(
        r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Story Self="s1">
          <ParagraphStyleRange><CharacterStyleRange><Content>a</Content>{}{}{}</CharacterStyleRange></ParagraphStyleRange>
        </Story></idPkg:Story>"#,
        rect("Note", ""),
        rect("Note", r#"SpineRelative="false""#),
        rect("$ID/[None]", ""),
    );
    let spine = |d: &Document| -> Vec<bool> {
        let s = d.stories.values().find(|s| !s.objects.is_empty()).unwrap();
        s.objects.iter().map(|o| matches!(o.position, AnchorPosition::Custom { spine_relative: true, .. })).collect()
    };
    let d = import_idml(&fixture_with_table_styles(&styles, &story)).unwrap();
    assert_eq!(spine(&d), [true, false, false]);
    assert_eq!(spine(&import_idml(&export_idml(&d)).unwrap()), [true, false, false]);
}

/// IDML measures a left-aligned anchored object's X offset leftwards; the document model and
/// composition measure every X offset rightwards.
#[test]
fn left_aligned_anchored_x_offsets_flip_at_the_idml_boundary() {
    use designcraft_doc::AnchorPosition;
    let rect = |id: &str, align: &str, x: &str| {
        format!(
            r#"<Rectangle Self="{id}" ItemTransform="1 0 0 1 0 0">
              <Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
                <PathPointType Anchor="0 0"/><PathPointType Anchor="0 10"/><PathPointType Anchor="10 10"/><PathPointType Anchor="10 0"/>
              </PathPointArray></GeometryPathType></PathGeometry></Properties>
              <AnchoredObjectSetting AnchoredPosition="Anchored" AnchorPoint="TopLeftAnchor" HorizontalReferencePoint="TextFrame"
                HorizontalAlignment="{align}" VerticalAlignment="TopAlign" VerticalReferencePoint="PageEdge" AnchorXoffset="{x}" AnchorYoffset="0"/>
            </Rectangle>"#
        )
    };
    let story = format!(
        r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Story Self="s1">
          <ParagraphStyleRange><CharacterStyleRange><Content>a</Content>{}{}{}</CharacterStyleRange></ParagraphStyleRange>
        </Story></idPkg:Story>"#,
        rect("left", "LeftAlign", "-108"),
        rect("zero", "LeftAlign", "0"),
        rect("center", "CenterAlign", "116"),
    );
    let d = import_idml(&fixture_with_story(&story)).unwrap();
    let x_offsets = |d: &Document| -> Vec<f64> {
        let s = d.stories.values().find(|s| !s.objects.is_empty()).unwrap();
        s.objects.iter().map(|o| if let AnchorPosition::Custom { x_offset, .. } = o.position { x_offset } else { f64::NAN }).collect()
    };
    assert_eq!(x_offsets(&d), [108.0, 0.0, 116.0]);
    // The left-aligned object lands 108 pt right of the reference point.
    let sid = d.stories.iter().find(|(_, s)| !s.objects.is_empty()).map(|(id, _)| *id).unwrap();
    let cs = designcraft_compose::compose_story(&d, sid, &Default::default());
    let placed: Vec<_> = cs.frames.iter().flat_map(|f| &f.objects).collect();
    let x = |i: usize| placed.iter().find(|o| o.index == i).unwrap().origin.x;
    assert!((x(0) - x(1) - 108.0).abs() < 1e-6, "{} {}", x(0), x(1));
    // Export writes the original IDML values back.
    let bytes = export_idml(&d);
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut stories = String::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        if f.name().starts_with("Stories/") {
            std::io::Read::read_to_string(&mut f, &mut stories).unwrap();
        }
    }
    for v in [r#"AnchorXoffset="-108""#, r#"AnchorXoffset="0""#, r#"AnchorXoffset="116""#] {
        assert!(stories.contains(v), "{v} missing");
    }
    assert_eq!(x_offsets(&import_idml(&export_idml(&d)).unwrap()), [108.0, 0.0, 116.0]);
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
fn composers_import_by_kind_and_round_trip() {
    use designcraft_doc::Composer as C;
    let para = |composer: &str, text: &str| {
        format!(
            r#"<ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/[No paragraph style]" Composer="{composer}"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>{text}</Content><Br /></CharacterStyleRange></ParagraphStyleRange>"#
        )
    };
    let cases = [
        ("HL Composer", C::Paragraph),
        ("HL Single Optyca", C::SingleLine),
        ("HL Composer Optyca", C::Paragraph),
        ("HL Composer J", C::Japanese),
        ("$ID/HL Single J", C::JapaneseSingleLine),
    ];
    let body: String = cases.iter().enumerate().map(|(i, (name, _))| para(name, &format!("p{i}"))).collect();
    let story =
        format!(r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Story Self="s1">{body}</Story></idPkg:Story>"#);
    let doc = import_idml(&fixture_with_story(&story)).unwrap();
    let back = import_idml(&export_idml(&doc)).unwrap();
    for d in [&doc, &back] {
        let st = d.stories.values().find(|s| s.text.contains("p0")).unwrap();
        let got: Vec<_> = st.paras.iter().take(cases.len()).map(|p| p.para.composer).collect();
        assert_eq!(got, cases.iter().map(|(_, c)| Some(*c)).collect::<Vec<_>>());
    }
}

#[test]
fn text_stroke_alignment_imports_as_stroke_behind_fill_and_round_trips() {
    let story = r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Story Self="s1">
      <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/[No paragraph style]">
        <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" StrokeColor="Color/Black" StrokeWeight="2" StrokeAlignment="OutsideAlignment"><Content>Behind</Content></CharacterStyleRange>
        <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" StrokeColor="Color/Black" StrokeAlignment="CenterAlignment"><Content>Over</Content></CharacterStyleRange>
        <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>Plain</Content></CharacterStyleRange>
      </ParagraphStyleRange></Story></idPkg:Story>"#;
    let flags = |d: &Document| -> Vec<(String, Option<bool>)> {
        let st = d.stories.values().find(|s| s.text.contains("Behind")).unwrap();
        st.runs().map(|(r, f)| (st.text[r].to_string(), f.over.stroke_behind_fill)).filter(|(t, _)| !t.trim().is_empty()).collect()
    };
    let want = vec![("Behind".to_string(), Some(true)), ("Over".to_string(), Some(false)), ("Plain".to_string(), None)];
    let doc = import_idml(&fixture_with_story(story)).unwrap();
    assert_eq!(flags(&doc), want);
    let back = import_idml(&export_idml(&doc)).unwrap();
    assert_eq!(flags(&back), want);
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
      <CompositeFontEntry Self="cf2" Name="Digits" CustomCharacters="0123456789" FontStyle="Regular" RelativeSize="80" BaselineShift="10"><Properties><AppliedFont type="string">$ID/Source Sans 3</AppliedFont></Properties></CompositeFontEntry></CompositeFont>
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
fn cjk_mojikumi_overrides_and_priority_round_trip() {
    let map = DESIGNMAP.replace("</Document>", r#"<MojikumiTable Self="uMojikumi1" Name="Spacing" BasedOnMojikumiSet="SimpChineseDefault"><Properties><OverrideMojikumiAkiList>
    <OverrideMojikumiAkiType TargetMojikumiClass="1" SideMojikumiClass="23" SideIsAfterTarget="false" Minimum="-0.1" Desired="0.25" Maximum="0.5" CompressionPriority="3" AkiDoesNotFloat="true"/>
    </OverrideMojikumiAkiList></Properties></MojikumiTable></Document>"#);
    let story = STORY.replace("<ParagraphStyleRange ", "<ParagraphStyleRange Mojikumi=\"uMojikumi1\" KinsokuType=\"KinsokuPushOutFirst\" ");
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

/// A custom table with no `BasedOnMojikumiSet` keeps its own rows on the
/// default half-em set, and the attribute stays absent on export.
#[test]
fn cjk_mojikumi_table_without_base_set_applies_its_rows() {
    let map = DESIGNMAP.replace("</Document>", r#"<MojikumiTable Self="uMojikumi2" Name="Baseless"><Properties><OverrideMojikumiAkiList>
    <OverrideMojikumiAkiType TargetMojikumiClass="12" SideMojikumiClass="18" SideIsAfterTarget="true" Minimum="0.25" Desired="0.75" Maximum="1" CompressionPriority="2" AkiDoesNotFloat="false"/>
    </OverrideMojikumiAkiList></Properties></MojikumiTable></Document>"#);
    let story = STORY.replace("<ParagraphStyleRange ", "<ParagraphStyleRange Mojikumi=\"uMojikumi2\" ");
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
    let name = d.stories.values().flat_map(|s| &s.paras).find_map(|p| p.para.mojikumi.clone()).unwrap();
    assert_eq!(name, "MojikumiTable/Baseless");
    let rules = designcraft_doc::mojikumi::Rules::resolve(&d.styles, &name).unwrap().unwrap();
    assert!(rules.assumed_base());
    let pair = rules.pair(12, 18);
    assert_eq!((pair.min, pair.desired, pair.max, pair.priority), (0.25, 0.75, 1.0, 2));
    // Pairs the table does not list use the half-em set: no blank at line start.
    assert_eq!(rules.pair(22, designcraft_doc::mojikumi::class('（')).desired, 0.0);
    let back = import_idml(&export_idml(&d)).unwrap();
    assert_eq!(back.styles.mojikumi_tables, d.styles.mojikumi_tables);
}

/// `BasedOnMojikumiSet="Nothing"` names no base set: the table resolves as one
/// without the attribute, and the value survives export.
#[test]
fn cjk_mojikumi_table_based_on_nothing_matches_a_table_without_base_set() {
    let row = r#"<OverrideMojikumiAkiType TargetMojikumiClass="12" SideMojikumiClass="18" SideIsAfterTarget="true" Minimum="0.25" Desired="0.75" Maximum="1" CompressionPriority="2" AkiDoesNotFloat="false"/>"#;
    let map = DESIGNMAP.replace("</Document>", &format!(r#"<MojikumiTable Self="uMojikumi2" Name="Baseless"><Properties><OverrideMojikumiAkiList>{row}</OverrideMojikumiAkiList></Properties></MojikumiTable>
    <MojikumiTable Self="uMojikumi3" Name="NoBase" BasedOnMojikumiSet="Nothing"><Properties><OverrideMojikumiAkiList>{row}</OverrideMojikumiAkiList></Properties></MojikumiTable></Document>"#));
    let story = STORY.replace("<ParagraphStyleRange ", "<ParagraphStyleRange Mojikumi=\"uMojikumi3\" ");
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
    let name = d.stories.values().flat_map(|s| &s.paras).find_map(|p| p.para.mojikumi.clone()).unwrap();
    assert_eq!(name, "MojikumiTable/NoBase");
    let nothing = designcraft_doc::mojikumi::Rules::resolve(&d.styles, &name).unwrap().unwrap();
    let missing = designcraft_doc::mojikumi::Rules::resolve(&d.styles, "MojikumiTable/Baseless").unwrap().unwrap();
    assert!(nothing.assumed_base());
    let close = designcraft_doc::mojikumi::class('。');
    for (l, r) in [(12, 18), (22, designcraft_doc::mojikumi::class('（')), (close, 22), (close, 12)] {
        let (a, b) = (missing.pair(l, r), nothing.pair(l, r));
        assert_eq!((a.min, a.desired, a.max, a.priority), (b.min, b.desired, b.max, b.priority), "{l}/{r}");
    }
    let back = import_idml(&export_idml(&d)).unwrap();
    assert_eq!(back.styles.mojikumi_tables, d.styles.mojikumi_tables);
    assert_eq!(back.styles.mojikumi_tables[1].based_on, "Nothing");
}

/// The built-in `LineEndAllOneEmEnum` set applies directly and as a custom
/// table's base: closing punctuation at line end keeps its full em.
#[test]
fn cjk_mojikumi_line_end_all_one_em_set_applies_and_round_trips() {
    let map = DESIGNMAP.replace("</Document>", r#"<MojikumiTable Self="uMojikumi4" Name="OneEm" BasedOnMojikumiSet="LineEndAllOneEmEnum"><Properties><OverrideMojikumiAkiList>
    <OverrideMojikumiAkiType TargetMojikumiClass="12" SideMojikumiClass="18" SideIsAfterTarget="true" Minimum="0.25" Desired="0.75" Maximum="1" CompressionPriority="2" AkiDoesNotFloat="false"/>
    </OverrideMojikumiAkiList></Properties></MojikumiTable></Document>"#);
    let story = STORY.replacen("<ParagraphStyleRange ", "<ParagraphStyleRange Mojikumi=\"$ID/LineEndAllOneEmEnum\" ", 1);
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
    let name = d.stories.values().flat_map(|s| &s.paras).find_map(|p| p.para.mojikumi.clone()).unwrap();
    let close = designcraft_doc::mojikumi::class('。');
    for name in [name.as_str(), "MojikumiTable/OneEm"] {
        let rules = designcraft_doc::mojikumi::Rules::resolve(&d.styles, name).unwrap().unwrap();
        assert!(!rules.assumed_base(), "{name}");
        assert_eq!(rules.pair(close, 22).desired, 0.5, "{name}");
        assert_eq!(rules.pair(22, designcraft_doc::mojikumi::class('（')).desired, 0.0, "{name}");
    }
    let back = import_idml(&export_idml(&d)).unwrap();
    assert_eq!(back.styles.mojikumi_tables, d.styles.mojikumi_tables);
    let back_name = back.stories.values().flat_map(|s| &s.paras).find_map(|p| p.para.mojikumi.clone()).unwrap();
    assert_eq!(back_name, name);
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

/// Synthetic forward references: both a body anchor and a footnote anchor own
/// a group containing a text frame whose story is serialized later.
#[test]
fn round_trips_anchored_text_frames_in_body_and_footnotes() {
    use designcraft_doc::{AnchorPosition, AnchoredObject, Content, Item, ItemId};
    use std::sync::Arc;
    let mut d = Document::new(&NewDocument::default());
    let layer = d.default_layer();
    let (_, body) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(20.0, 20.0, 400.0, 600.0), layer, "Body", ParaFormat::default()).unwrap();
    d.story_mut(body).unwrap().insert_note(4, "Note", ParaFormat::default());
    for in_note in [false, true] {
        let (frame, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 60.0, 30.0), layer, "Symbol", ParaFormat::default()).unwrap();
        let child = d.item(frame).unwrap().clone();
        d.spread_mut(SpreadRef::Doc(0)).unwrap().items.retain(|i| i.id != frame);
        let mut group = Item::new(ItemId(d.alloc()), layer, Shape::Group, Default::default());
        group.content = Content::Group { items: vec![Arc::new(child)] };
        let anchor = AnchoredObject::new(group, AnchorPosition::Inline { y_offset: 2.0 });
        let story = d.story_mut(body).unwrap();
        if in_note {
            Arc::make_mut(&mut story.notes[0]).text.insert_object(0, anchor);
        } else {
            story.insert_object(0, anchor);
        }
    }
    d.check().unwrap();
    for _ in 0..2 {
        d = import_idml(&export_idml(&d)).unwrap();
        d.check().unwrap();
        assert_eq!(d.stories.len(), 3);
        let body = d.stories.values().find(|s| !s.notes.is_empty()).unwrap();
        assert_eq!(body.notes.len(), 1);
        assert_eq!(body.objects.len(), 1);
        assert_eq!(body.notes[0].text.objects.len(), 1);
        let children: Vec<_> = d.anchored_items().into_iter().filter(|(_, i)| i.text_frame().is_some()).collect();
        assert_eq!(children.len(), 2);
        for (_, item) in children {
            let story = d.story(item.text_frame().unwrap().story).unwrap();
            assert_eq!(story.text, "Symbol");
            assert_eq!(story.frames, vec![item.id]);
        }
    }
}

#[test]
fn composite_references_resolve_by_identity_and_export_as_objects() {
    let map = DESIGNMAP.replace("</Document>", r#"
      <CompositeFont Self="CompositeFont/opaque%3a17" Name="Mixed%3a literal">
        <CompositeFontEntry Name="Base" FontStyle="$ID/Regular"><Properties><AppliedFont type="string">Source Serif 4</AppliedFont></Properties></CompositeFontEntry>
        <CompositeFontEntry Name="Digits" CustomCharacters="0123456789" FontStyle="$ID/Bold" RelativeSize="80" HorizontalScale="90" VerticalScale="110" BaselineShift="5" ScaleOption="false"><Properties><AppliedFont type="string">Source Sans 3</AppliedFont></Properties></CompositeFontEntry>
      </CompositeFont></Document>"#);
    let styles = STYLES
        .replace("<AppliedFont type=\"string\">Source Serif 4</AppliedFont>", "<AppliedFont type=\"object\">CompositeFont/opaque%3a17</AppliedFont>");
    let story = r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Story Self="s1"><ParagraphStyleRange><CharacterStyleRange><Content>A1</Content><Footnote><ParagraphStyleRange><CharacterStyleRange><Content>B2</Content></CharacterStyleRange></ParagraphStyleRange></Footnote></CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>"#;
    let bytes = zip_files(&[
        ("designmap.xml", &map),
        ("Resources/Styles.xml", &styles),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Preferences.xml", PREFS),
        ("MasterSpreads/MasterSpread_m1.xml", MASTER),
        ("Spreads/Spread_sp1.xml", SPREAD),
        ("Stories/Story_s1.xml", story),
    ]);
    let d = import_idml(&bytes).unwrap();
    let f = &d.styles.composite_fonts[0];
    assert_eq!(f.name, "Mixed%3a literal", "display names are not encoded identifiers");
    let body = d.stories.values().find(|s| !s.notes.is_empty()).unwrap();
    for text in [body.as_ref(), &body.notes[0].text] {
        let (_, base) = d.styles.resolve_para(&text.paras[0]);
        let p = d.styles.resolve_char(&base, &text.chars[0].format);
        assert_eq!(d.styles.composite_font(&p.font_family), Some(f));
    }
    let output = export_idml(&d);
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&output)).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("Resources/Styles.xml").unwrap(), &mut xml).unwrap();
    assert!(xml.contains("<AppliedFont type=\"object\">CompositeFont/Mixed%253a literal</AppliedFont>"), "{xml}");
    let back = import_idml(&output).unwrap();
    assert_eq!(back.styles.composite_fonts, d.styles.composite_fonts);
}

#[test]
fn malformed_mojikumi_rows_are_not_silently_coerced() {
    let good = r#"<OverrideMojikumiAkiType TargetMojikumiClass="12" SideMojikumiClass="18" SideIsAfterTarget="true" Minimum="0" Desired="0.25" Maximum="0.5" CompressionPriority="3" AkiDoesNotFloat="false"/>"#;
    for (key, value) in [
        ("TargetMojikumiClass", "12.9"),
        ("SideMojikumiClass", "65536"),
        ("Minimum", "NaN"),
        ("Desired", "oops"),
        ("Minimum", "0.4"),
        ("Maximum", "0.1"),
        ("Maximum", "101"),
        ("CompressionPriority", "10"),
        ("AkiDoesNotFloat", "maybe"),
    ] {
        let mut row = crate::xml::parse(good.as_bytes()).unwrap();
        row.set(key, value);
        let mut xml = String::new();
        row.write(&mut xml, 0);
        let map = DESIGNMAP.replace("</Document>", &format!("<MojikumiTable Name=\"Bad\" BasedOnMojikumiSet=\"SimpChineseDefault\"><Properties><OverrideMojikumiAkiList>{xml}</OverrideMojikumiAkiList></Properties></MojikumiTable></Document>"));
        let bytes = zip_files(&[
            ("designmap.xml", &map),
            ("Resources/Graphic.xml", GRAPHIC),
            ("Resources/Styles.xml", STYLES),
            ("Resources/Preferences.xml", PREFS),
            ("MasterSpreads/MasterSpread_m1.xml", MASTER),
            ("Spreads/Spread_sp1.xml", SPREAD),
            ("Stories/Story_s1.xml", STORY),
        ]);
        if matches!((key, value), ("Minimum", "0.4") | ("Maximum", "0.1" | "101") | ("CompressionPriority", "10")) {
            // Finite, representable rules survive import even if composition
            // cannot apply them. The resolver supplies a diagnostic instead.
            let document = import_idml(&bytes).unwrap();
            document.check().unwrap();
            assert!(designcraft_doc::mojikumi::Rules::resolve(&document.styles, "MojikumiTable/Bad").is_err());
            let restored = import_idml(&export_idml(&document)).unwrap();
            assert_eq!(restored.styles.mojikumi_tables, document.styles.mojikumi_tables);
        } else {
            let err = import_idml(&bytes).unwrap_err().to_string();
            assert!(err.contains("Mojikumi"), "{key}={value}: {err}");
        }
    }
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
fn paragraph_border_shading_span_and_list_options_import_and_round_trip() {
    use designcraft_doc::{BoxBottom, BoxTop, BoxWidth, Cap, Join, ListAlign, StrokeType};
    use designcraft_geom::corners::{Corner, CornerShape};
    let story = r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Story Self="s1">
      <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/[No paragraph style]" ParagraphBorderOn="true"
        ParagraphBorderStrokeEndCap="RoundEndCap" ParagraphBorderStrokeEndJoin="BevelEndJoin"
        ParagraphBorderTopLeftCornerOption="RoundedCorner" ParagraphBorderTopLeftCornerRadius="6"
        ParagraphBorderTopRightCornerOption="BevelCorner" ParagraphBorderTopRightCornerRadius="4"
        ParagraphBorderBottomRightCornerOption="None" ParagraphBorderBottomRightCornerRadius="12"
        ParagraphBorderBottomLeftCornerOption="InsetCorner" ParagraphBorderBottomLeftCornerRadius="3"
        ParagraphBorderWidth="TextWidth" ParagraphBorderTopOrigin="EmBoxTopOrigin" ParagraphBorderBottomOrigin="EmBoxBottomOrigin"
        ParagraphBorderDisplayIfSplits="true" MergeConsecutiveParaBorders="false" ParagraphBorderGapTint="50"
        ParagraphShadingOn="true" ParagraphShadingTopLeftCornerOption="RoundedCorner" ParagraphShadingTopLeftCornerRadius="8"
        ParagraphShadingTopRightCornerOption="RoundedCorner" ParagraphShadingTopRightCornerRadius="8"
        ParagraphShadingWidth="TextWidth" ParagraphShadingTopOrigin="BaselineTopOrigin" ParagraphShadingBottomOrigin="BaselineBottomOrigin"
        ParagraphShadingClipToFrame="true" ParagraphShadingSuppressPrinting="true"
        SpanColumnType="SpanColumns" SpanColumnMinSpaceBefore="6" SpanColumnMinSpaceAfter="9"
        BulletsAndNumberingListType="NumberedList" NumberingLevel="2" NumberingAlignment="RightAlign" BulletsAlignment="CenterAlign"
        NumberingApplyRestartPolicy="false">
        <Properties>
          <ParagraphBorderType type="object">StrokeStyle/$ID/Dashed</ParagraphBorderType>
          <ParagraphBorderGapColor type="object">Swatch/None</ParagraphBorderGapColor>
          <SpanSplitColumnCount type="enumeration">All</SpanSplitColumnCount>
          <NumberingRestartPolicies RestartPolicy="SpecificPreviousLevel" LowerLevel="1" UpperLevel="1"/>
          <BulletsFont type="string">Courier New</BulletsFont>
          <BulletsFontStyle type="string">Bold</BulletsFontStyle>
        </Properties>
        <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>Boxed</Content></CharacterStyleRange>
      </ParagraphStyleRange></Story></idPkg:Story>"#;
    let doc = import_idml(&fixture_with_story(story)).unwrap();
    let para = |d: &Document| d.stories.values().find(|s| s.text.contains("Boxed")).unwrap().paras[0].para.clone();
    let a = para(&doc);
    let corner = |shape, size| Corner { shape, size };
    assert_eq!(a.border_type, Some(StrokeType::Dashed { pattern: vec![] }));
    assert_eq!((a.border_cap, a.border_join), (Some(Cap::Round), Some(Join::Bevel)));
    assert_eq!(
        a.border_corners,
        Some([corner(CornerShape::Rounded, 6.0), corner(CornerShape::Bevel, 4.0), corner(CornerShape::None, 12.0), corner(CornerShape::Inset, 3.0)])
    );
    assert_eq!((a.border_width, a.border_top, a.border_bottom), (Some(BoxWidth::Text), Some(BoxTop::EmBox), Some(BoxBottom::EmBox)));
    assert_eq!((a.border_display_if_splits, a.border_merge, a.border_gap_tint), (Some(true), Some(false), Some(0.5)));
    assert!(a.border_gap_color.is_some());
    assert_eq!(a.shading_corners.map(|c| c[1]), Some(corner(CornerShape::Rounded, 8.0)));
    assert_eq!((a.shading_width, a.shading_top, a.shading_bottom), (Some(BoxWidth::Text), Some(BoxTop::Baseline), Some(BoxBottom::Baseline)));
    assert_eq!((a.shading_clip, a.shading_nonprinting), (Some(true), Some(true)));
    assert_eq!((a.span_space_before, a.span_space_after), (Some(6.0), Some(9.0)));
    assert_eq!((a.list_level, a.number_align, a.bullet_align), (Some(2), Some(ListAlign::Right), Some(ListAlign::Center)));
    assert_eq!((a.restart_numbers, a.restart_after_level), (Some(false), Some(1)));
    assert_eq!((a.bullet_font.as_deref(), a.bullet_font_style.as_deref()), (Some("Courier New"), Some("Bold")));
    let b = para(&import_idml(&export_idml(&doc)).unwrap());
    let pick = |p: &designcraft_doc::ParaAttrs| {
        let border = (&p.border_type, p.border_cap, p.border_join, p.border_corners, p.border_width, p.border_top, p.border_bottom);
        let more = (p.border_display_if_splits, p.border_merge, &p.border_gap_color, p.border_gap_tint);
        let shading = (p.shading_corners, p.shading_width, p.shading_top, p.shading_bottom, p.shading_clip, p.shading_nonprinting);
        let list = (p.span_space_before, p.span_space_after, p.list_level, p.number_align, p.bullet_align, p.restart_numbers);
        format!("{:?}", (border, more, shading, list, p.restart_after_level, &p.bullet_font, &p.bullet_font_style))
    };
    assert_eq!(pick(&b), pick(&a), "the attributes survive export and import");
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
        assert_eq!(kanji.number_style, designcraft_doc::NumberStyle::Kanji);
        let (point, _) = d.styles.resolve_para_style("Text/Point");
        assert_eq!(point.bullet_label(), "\u{25A0}\u{2002}");
    };
    let d = import_idml(&bytes).unwrap();
    check(&d);
    check(&import_idml(&export_idml(&d)).unwrap());
}

/// InDesign can write `AppliedFont` with its `$ID/` prefix (`$ID/Source Serif 4`). The text is
/// set in that family instead of counting as a missing font drawn in the fallback.
#[test]
fn applied_font_with_an_id_prefix_names_the_family() {
    let styles = STYLES.replace(">Source Serif 4</AppliedFont>", ">$ID/Source Serif 4</AppliedFont>");
    let story = STORY.replace(
        r#"<CharacterStyleRange AppliedCharacterStyle="CharacterStyle/Strong" PointSize="14">"#,
        r#"<CharacterStyleRange AppliedCharacterStyle="CharacterStyle/Strong" PointSize="14"><Properties><AppliedFont type="string">$ID/Source Serif 4</AppliedFont></Properties>"#,
    );
    let bytes = zip_files(&[
        ("designmap.xml", DESIGNMAP),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Styles.xml", &styles),
        ("Resources/Preferences.xml", PREFS),
        ("MasterSpreads/MasterSpread_m1.xml", MASTER),
        ("Spreads/Spread_sp1.xml", SPREAD),
        ("Stories/Story_s1.xml", &story),
    ]);
    let d = import_idml_with(&bytes, &|_| None).unwrap();
    let fonts: Vec<&str> = d.styles.paragraph.iter().filter_map(|s| s.chars.font_family.as_deref()).collect();
    assert!(!fonts.is_empty() && fonts.iter().all(|f| *f == "Source Serif 4"), "{fonts:?}");
    let (sid, story) = d.stories.iter().next().unwrap();
    let bold = story.runs().find(|(r, _)| story.text[r.clone()].starts_with("bold")).unwrap().1;
    assert_eq!(bold.over.font_family.as_deref(), Some("Source Serif 4"));
    // The bundled family draws the text (an unknown family would fall back to Source Sans 3).
    let cs = designcraft_compose::compose_story(&d, *sid, &Default::default());
    let first_para = story.text.find('\n').unwrap();
    let families: Vec<&str> = cs
        .frames
        .iter()
        .flat_map(|f| &f.lines)
        .flat_map(|l| &l.glyphs)
        .filter(|g| g.visible && g.len > 0 && g.byte < first_para)
        .map(|g| g.face.get().family.as_str())
        .collect();
    assert!(!families.is_empty() && families.iter().all(|f| *f == "Source Serif 4"), "{families:?}");
}

#[test]
fn list_tab_position_round_trips_as_a_tab_stop() {
    use designcraft_doc::{ListType, ParaAttrs, ParagraphStyle, TabAlign, TabStop};
    let stop = |position, align| TabStop { position, align, leader: String::new(), align_on: String::new() };
    let mut d = Document::new(&NewDocument::default());
    std::sync::Arc::make_mut(&mut d.styles).paragraph.push(ParagraphStyle {
        name: "List".into(),
        based_on: None,
        next_style: None,
        para: ParaAttrs { list_type: Some(ListType::Bullets), list_tab: Some(Some(24.0)), ..Default::default() },
        chars: CharAttrs::default(),
        shortcut: String::new(),
    });
    let local = ParaAttrs {
        list_type: Some(ListType::Bullets),
        list_tab: Some(Some(36.0)),
        tabs: Some(vec![stop(100.0, TabAlign::Right)]),
        ..Default::default()
    };
    let ended = ParaAttrs { list_type: Some(ListType::None), ..Default::default() };
    let lid = d.default_layer();
    for (i, (text, style, para)) in
        [("Local\tx", "", local), ("Styled\ty", "List", ParaAttrs::default()), ("Ended\tz", "List", ended)].into_iter().enumerate()
    {
        let pf = ParaFormat { style: if style.is_empty() { ParaFormat::default().style } else { style.into() }, para, ..Default::default() };
        let y = 40.0 + 100.0 * i as f64;
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(40.0, y, 300.0, y + 80.0), lid, text, pf).unwrap();
    }
    let back = import_idml(&export_idml(&d)).unwrap();
    let tabs = |prefix: &str| {
        let s = back.stories.values().find(|s| s.text.starts_with(prefix)).unwrap();
        back.styles.resolve_para(&s.paras[0]).0.tabs
    };
    assert_eq!(tabs("Local"), vec![stop(36.0, TabAlign::Left), stop(100.0, TabAlign::Right)]);
    assert_eq!(tabs("Styled"), vec![stop(24.0, TabAlign::Left)]);
    assert_eq!(tabs("Ended"), vec![], "a paragraph that ends the list has no list tab stop");
}

#[test]
fn round_trips_the_zero_point_and_ruler_origin() {
    let mut d = Document::new(&NewDocument { pages: 3, ..Default::default() });
    d.settings.zero_point = [36.0, 18.5];
    let back = import_idml(&export_idml(&d)).unwrap();
    assert_eq!(back.settings.zero_point, [36.0, 18.5]);
    for origin in [designcraft_doc::RulerOrigin::Page, designcraft_doc::RulerOrigin::Spine, designcraft_doc::RulerOrigin::Spread] {
        d.settings.ruler_origin = origin;
        assert_eq!(import_idml(&export_idml(&d)).unwrap().settings.ruler_origin, origin);
    }
    // A hostile ZeroPoint lands on the pasteboard; a malformed one is ignored.
    let fixture = |zp: &str| {
        let designmap = format!(
            r#"<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" Self="d" ZeroPoint="{zp}">
              <idPkg:Spread src="Spreads/Spread_s.xml"/>
            </Document>"#
        );
        let spread = r#"<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
              <Spread Self="s"><Page Self="p" GeometricBounds="0 0 100 100" ItemTransform="1 0 0 1 0 0"/></Spread>
            </idPkg:Spread>"#;
        import_idml(&zip_files(&[("designmap.xml", &designmap), ("Spreads/Spread_s.xml", spread)])).unwrap()
    };
    let [x, y] = fixture("1e300 -1e300").settings.zero_point;
    assert!(x.is_finite() && y.is_finite() && x < 1e4 && y > -1e4, "{x} {y}");
    assert_eq!(fixture("12").settings.zero_point, [0.0, 0.0]);
    assert_eq!(fixture("NaN 4").settings.zero_point, [0.0, 0.0]);
}

#[test]
fn round_trips_lock_column_guides() {
    let mut d = Document::new(&NewDocument::default());
    assert!(zip_text(&export_idml(&d), "Resources/Preferences.xml").contains(r#"ColumnGuideLocked="true""#));
    assert!(import_idml(&export_idml(&d)).unwrap().settings.column_guides_locked);
    d.settings.column_guides_locked = false;
    let bytes = export_idml(&d);
    let prefs = zip_text(&bytes, "Resources/Preferences.xml");
    assert!(prefs.contains(r#"ColumnGuideLocked="false""#), "{prefs}");
    assert!(!import_idml(&bytes).unwrap().settings.column_guides_locked);
}

#[test]
fn kenten_shatai_and_grid_settings_import_and_round_trip() {
    use designcraft_doc::cjk::CharacterAlignment;
    use designcraft_doc::cjk_settings::{AdornmentOverprint, KentenAlignment, KentenKind, KentenPosition};
    let story = r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Story Self="s1">
      <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/[No paragraph style]" AutoTcy="3" AutoTcyIncludeRoman="true" RotateSingleByteCharacters="true" AllowArbitraryHyphenation="true" GridAlignment="AlignEmCenter" GridAlignFirstLineOnly="true" GridGyoudori="2" ParagraphGyoudori="true">
        <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" KentenKind="KentenSmallBlackCircle" KentenFontSize="6" KentenXScale="80" KentenYScale="120" KentenPlacement="1.5" KentenPosition="BelowLeft" KentenAlignment="AlignKentenLeft" KentenCharacterSet="CharacterInput" KentenTint="50" KentenStrokeTint="-1" KentenWeight="0.5" KentenOverprintFill="OverprintOn" KentenOverprintStroke="Auto" ShataiMagnification="20" ShataiDegreeAngle="6000" ShataiAdjustRotation="true" ShataiAdjustTsume="false"><Properties><KentenFont type="string">$ID/</KentenFont><KentenFontStyle type="enumeration">Nothing</KentenFontStyle><KentenFillColor type="string">Text Color</KentenFillColor></Properties><Content>甲乙</Content></CharacterStyleRange>
        <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" KentenKind="Custom" KentenCustomCharacter="★"><Content>丙</Content></CharacterStyleRange>
      </ParagraphStyleRange></Story></idPkg:Story>"#;
    let doc = import_idml(&fixture_with_story(story)).unwrap();
    let st = doc.stories.values().find(|s| s.text.contains("甲乙")).unwrap();
    let runs: Vec<CharAttrs> = st.runs().map(|(_, f)| f.over.clone()).collect();
    let a = &runs[0];
    assert_eq!((a.kenten, a.kenten_kind, a.kenten_character.as_deref()), (Some(true), Some(KentenKind::SmallBlackCircle), Some("\u{2022}")));
    assert_eq!((a.kenten_size, a.kenten_x_scale, a.kenten_y_scale, a.kenten_distance), (Some(Some(6.0)), Some(0.8), Some(1.2), Some(1.5)));
    assert_eq!((a.kenten_position, a.kenten_alignment), (Some(KentenPosition::BelowLeft), Some(KentenAlignment::Start)));
    assert_eq!((a.kenten_font.as_deref(), a.kenten_font_style.as_deref(), a.kenten_fill.as_deref()), (Some(""), Some(""), Some("")));
    assert_eq!((a.kenten_fill_tint, a.kenten_stroke_tint, a.kenten_stroke_weight), (Some(Some(0.5)), Some(None), Some(Some(0.5))));
    assert_eq!((a.kenten_overprint_fill, a.kenten_overprint_stroke), (Some(AdornmentOverprint::On), Some(AdornmentOverprint::Auto)));
    assert_eq!(a.kenten_character_set.as_deref(), Some("CharacterInput"));
    assert_eq!(
        (a.shatai_magnification, a.shatai_angle, a.shatai_adjust_rotation, a.shatai_adjust_tsume),
        (Some(20.0), Some(60.0), Some(true), Some(false))
    );
    let custom = runs.iter().find(|r| r.kenten_kind == Some(KentenKind::Custom)).unwrap();
    assert_eq!(custom.kenten_character.as_deref(), Some("★"));
    let p = &st.paras[0].para;
    assert_eq!((p.auto_tcy, p.auto_tcy_include_roman, p.rotate_roman, p.roman_word_break), (Some(3), Some(true), Some(true), Some(true)));
    assert_eq!((p.grid_align, p.grid_reference), (Some(designcraft_doc::GridAlign::FirstLineOnly), Some(CharacterAlignment::EmCenter)));
    assert_eq!((p.grid_gyoudori, p.paragraph_gyoudori), (Some(2), Some(true)));

    let back = import_idml(&export_idml(&doc)).unwrap();
    let st2 = back.stories.values().find(|s| s.text.contains("甲乙")).unwrap();
    let runs2: Vec<CharAttrs> = st2.runs().map(|(_, f)| f.over.clone()).collect();
    for (a, b) in runs.iter().zip(&runs2) {
        assert_eq!((a.kenten, a.kenten_kind, &a.kenten_character), (b.kenten, b.kenten_kind, &b.kenten_character));
        assert_eq!(
            (a.kenten_size, a.kenten_x_scale, a.kenten_y_scale, a.kenten_distance),
            (b.kenten_size, b.kenten_x_scale, b.kenten_y_scale, b.kenten_distance)
        );
        assert_eq!(
            (a.kenten_position, a.kenten_alignment, &a.kenten_character_set),
            (b.kenten_position, b.kenten_alignment, &b.kenten_character_set)
        );
        assert_eq!((&a.kenten_font, &a.kenten_font_style, &a.kenten_fill), (&b.kenten_font, &b.kenten_font_style, &b.kenten_fill));
        assert_eq!(
            (a.kenten_fill_tint, a.kenten_stroke_tint, a.kenten_stroke_weight),
            (b.kenten_fill_tint, b.kenten_stroke_tint, b.kenten_stroke_weight)
        );
        assert_eq!((a.kenten_overprint_fill, a.kenten_overprint_stroke), (b.kenten_overprint_fill, b.kenten_overprint_stroke));
        assert_eq!(
            (a.shatai_magnification, a.shatai_angle, a.shatai_adjust_rotation, a.shatai_adjust_tsume),
            (b.shatai_magnification, b.shatai_angle, b.shatai_adjust_rotation, b.shatai_adjust_tsume)
        );
    }
    let p2 = &st2.paras[0].para;
    assert_eq!(
        (p.auto_tcy, p.auto_tcy_include_roman, p.rotate_roman, p.roman_word_break),
        (p2.auto_tcy, p2.auto_tcy_include_roman, p2.rotate_roman, p2.roman_word_break)
    );
    assert_eq!(
        (p.grid_align, p.grid_reference, p.grid_gyoudori, p.paragraph_gyoudori),
        (p2.grid_align, p2.grid_reference, p2.grid_gyoudori, p2.paragraph_gyoudori)
    );
}

#[test]
fn ruby_settings_import_from_idml_and_round_trip() {
    use designcraft_doc::cjk_settings::AdornmentOverprint;
    use designcraft_doc::ruby::{RubyAlignment, RubyOverhang, RubyParentSpacing, RubyPosition, RubyType};
    // InDesign's names: RubyFlag is a number, colours and fonts are properties.
    let story = r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Story Self="s1">
      <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/[No paragraph style]">
        <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" RubyFlag="1" RubyString="かん　じ" RubyType="PerCharacterRuby" RubyAlignment="RubyCenter" RubyPosition="BelowLeft" RubyYOffset="-1.4173228" RubyFontSize="-1" RubyXScale="90" RubyParentOverhangAmount="RubyOverhangNoLimit" RubyParentSpacing="RubyParentBothSides" RubyAutoAlign="false" RubyAutoScaling="true" RubyParentScalingPercent="40" RubyAutoTcyDigits="2" RubyTint="-1" RubyOverprintFill="Auto"><Properties><RubyFont type="string">$ID/</RubyFont><RubyFontStyle type="enumeration">Nothing</RubyFontStyle><RubyFill type="string">Text Color</RubyFill></Properties><Content>漢字</Content></CharacterStyleRange>
      </ParagraphStyleRange></Story></idPkg:Story>"#;
    let doc = import_idml(&fixture_with_story(story)).unwrap();
    let a = doc.stories.values().find(|s| s.text.contains("漢字")).unwrap().runs().next().unwrap().1.over.clone();
    assert_eq!(a.ruby.as_deref(), Some("かん　じ"));
    assert_eq!(a.ruby_type, Some(RubyType::PerCharacter));
    assert_eq!(a.ruby_alignment, Some(RubyAlignment::Center));
    assert_eq!(a.ruby_position, Some(RubyPosition::BelowLeft));
    assert_eq!(a.ruby_y_offset, Some(-1.4173228));
    assert_eq!(a.ruby_font_size, Some(None));
    assert_eq!(a.ruby_x_scale, Some(0.9));
    assert_eq!(a.ruby_overhang_amount, Some(RubyOverhang::NoLimit));
    assert_eq!(a.ruby_parent_spacing, Some(RubyParentSpacing::BothSides));
    assert_eq!((a.ruby_auto_align, a.ruby_auto_scaling, a.ruby_scaling_min), (Some(false), Some(true), Some(0.4)));
    assert_eq!(a.ruby_auto_tcy_digits, Some(2));
    assert_eq!((a.ruby_font.as_deref(), a.ruby_font_style.as_deref(), a.ruby_fill.as_deref()), (Some(""), Some(""), Some("")));
    assert_eq!((a.ruby_fill_tint, a.ruby_overprint_fill), (Some(None), Some(AdornmentOverprint::Auto)));

    // Every setting survives export and import.
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(72.0, 72.0, 300.0, 200.0), lid, "漢字", ParaFormat::default()).unwrap();
    d.story_mut(sid).unwrap().format_chars(0.."漢字".len(), |f| {
        let o = &mut f.over;
        o.ruby = Some("かん　じ".into());
        o.ruby_type = Some(RubyType::PerCharacter);
        o.ruby_alignment = Some(RubyAlignment::OneAki);
        o.ruby_position = Some(RubyPosition::BelowLeft);
        o.ruby_x_offset = Some(1.5);
        o.ruby_y_offset = Some(-1.25);
        o.ruby_font = Some("Source Sans 3".into());
        o.ruby_font_style = Some("Bold".into());
        o.ruby_font_size = Some(Some(7.0));
        o.ruby_x_scale = Some(0.9);
        o.ruby_y_scale = Some(1.1);
        o.ruby_open_type_pro = Some(false);
        o.ruby_auto_tcy_digits = Some(3);
        o.ruby_auto_tcy_include_roman = Some(true);
        o.ruby_auto_tcy_auto_scale = Some(false);
        o.ruby_overhang = Some(true);
        o.ruby_overhang_amount = Some(RubyOverhang::HalfChar);
        o.ruby_parent_spacing = Some(RubyParentSpacing::EqualAki);
        o.ruby_auto_align = Some(false);
        o.ruby_auto_scaling = Some(true);
        o.ruby_scaling_min = Some(0.5);
        o.ruby_fill = Some("[Black]".into());
        o.ruby_fill_tint = Some(Some(0.5));
        o.ruby_stroke = Some(String::new());
        o.ruby_stroke_tint = Some(None);
        o.ruby_stroke_weight = Some(Some(0.25));
        o.ruby_overprint_fill = Some(AdornmentOverprint::On);
        o.ruby_overprint_stroke = Some(AdornmentOverprint::Off);
    });
    let a = d.stories[&sid].runs().next().unwrap().1.over.clone();
    let back = import_idml(&export_idml(&d)).unwrap();
    let b = back.stories.values().find(|s| s.text.contains("漢字")).unwrap().runs().next().unwrap().1.over.clone();
    macro_rules! same {
        ($($f:ident),*) => { $( assert_eq!(a.$f, b.$f, stringify!($f)); )* };
    }
    same!(
        ruby,
        ruby_type,
        ruby_alignment,
        ruby_position,
        ruby_x_offset,
        ruby_y_offset,
        ruby_font,
        ruby_font_style,
        ruby_font_size,
        ruby_x_scale,
        ruby_y_scale,
        ruby_open_type_pro,
        ruby_auto_tcy_digits,
        ruby_auto_tcy_include_roman,
        ruby_auto_tcy_auto_scale,
        ruby_overhang,
        ruby_overhang_amount,
        ruby_parent_spacing,
        ruby_auto_align,
        ruby_auto_scaling,
        ruby_scaling_min,
        ruby_fill,
        ruby_fill_tint,
        ruby_stroke,
        ruby_stroke_tint,
        ruby_stroke_weight,
        ruby_overprint_fill,
        ruby_overprint_stroke
    );
}

/// A file's tab list is sanitized as the Tabs commands check theirs: a hostile one imports
/// within the limits instead of failing the import.
#[test]
fn hostile_tab_lists_import_within_the_limits() {
    let item = |position: &str, leader: &str| {
        format!(
            r#"<ListItem type="record"><Alignment type="enumeration">LeftAlign</Alignment><AlignmentCharacter type="string">.</AlignmentCharacter><Leader type="string">{leader}</Leader><Position type="unit">{position}</Position></ListItem>"#
        )
    };
    let mut items: String = (0..500).rev().map(|i| item(&(i * 3).to_string(), "")).collect();
    items.push_str(&item("NaN", ""));
    items.push_str(&item("1e300", "0123456789"));
    let story = format!(
        r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Story Self="s1">
      <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/[No paragraph style]">
        <Properties><TabList type="list">{items}</TabList></Properties>
        <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>Many tabs</Content></CharacterStyleRange>
      </ParagraphStyleRange></Story></idPkg:Story>"#
    );
    let report = import_idml_report(&fixture_with_story(&story), &|_| None).unwrap();
    assert!(report.warnings.iter().any(|w| w.starts_with("tab lists repaired to fit the limits") && w.ends_with("): 1")), "{:?}", report.warnings);
    let doc = report.document;
    let tabs = doc.stories.values().find(|s| s.text.contains("Many tabs")).unwrap().paras[0].para.tabs.clone().unwrap();
    assert_eq!(tabs.len(), designcraft_doc::MAX_TAB_STOPS);
    assert!(tabs.iter().all(|t| t.position.is_finite() && t.leader.chars().count() <= designcraft_doc::MAX_TAB_LEADER));
    assert!(tabs.windows(2).all(|w| w[0].position < w[1].position), "sorted");
    assert_eq!((tabs[0].position, tabs[99].position), (0.0, 297.0), "the first stops by position");
    // A list within the limits, unsorted and with a char stop on `.`, imports without a warning.
    let fine = format!(
        r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Story Self="s1">
      <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/[No paragraph style]">
        <Properties><TabList type="list">{}{}<ListItem type="record"><Alignment type="enumeration">CharacterAlign</Alignment><AlignmentCharacter type="string">.</AlignmentCharacter><Leader type="string"></Leader><Position type="unit">40</Position></ListItem></TabList></Properties>
        <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>Few tabs</Content></CharacterStyleRange>
      </ParagraphStyleRange></Story></idPkg:Story>"#,
        item("90", "."),
        item("12", "")
    );
    let report = import_idml_report(&fixture_with_story(&fine), &|_| None).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn cell_edge_gap_colours_and_tints_import_and_round_trip_per_edge() {
    let styles = CASCADE_STYLES.replace(
        r#"TopEdgeStrokeType="StrokeStyle/$ID/Dashed""#,
        r#"TopEdgeStrokeType="StrokeStyle/$ID/Dashed" TopEdgeStrokeGapColor="Color/Paper" TopEdgeStrokeGapTint="60" BottomEdgeStrokeGapColor="Color/Brand""#,
    );
    let story = CASCADE_STORY.replace(
        r#"<Cell Name="1:1">"#,
        r#"<Cell Name="1:1" LeftEdgeStrokeType="StrokeStyle/$ID/Dotted" LeftEdgeStrokeGapColor="Color/Brand" LeftEdgeStrokeGapTint="30" RightEdgeStrokeGapColor="Color/Paper">"#,
    );
    let gaps = |d: &Document| {
        let base = d.styles.cell.iter().find(|s| s.name == "Base").unwrap().strokes.clone();
        let table = d.stories.values().flat_map(|s| s.tables.values()).next().unwrap();
        let cell = table.cell(1, 1).unwrap().strokes.clone();
        (base.map(|e| (e.gap_color, e.gap_tint)), cell.map(|e| (e.gap_color, e.gap_tint)))
    };
    let d = import_idml(&fixture_with_table_styles(&styles, &story)).unwrap();
    d.check().unwrap();
    let (style, cell) = gaps(&d);
    assert_eq!(style[0], (Some("[Paper]".to_string()), Some(0.6)));
    assert_eq!(style[2], (Some("Brand".to_string()), None));
    assert_eq!((style[1].clone(), style[3].clone()), ((None, None), (None, None)));
    assert_eq!(cell[1], ("Brand".to_string(), 0.3), "left");
    assert_eq!(cell[3], ("[Paper]".to_string(), 1.0), "right");
    assert_eq!(cell[0].0, "[Paper]", "the Base style's top gap colour reaches the cell");
    let back = import_idml(&export_idml(&d)).unwrap();
    assert_eq!(gaps(&back), (style, cell));
}

#[test]
fn em_box_first_baseline_offset_round_trips() {
    use designcraft_doc::FirstBaseline;
    let first = |d: &Document| d.spreads[0].items[0].text_frame().unwrap().options.first_baseline;
    let d = inset_fixture(r#"<TextFramePreference FirstBaselineOffset="EmboxHeight"/>"#, "");
    assert_eq!(first(&d), FirstBaseline::EmboxHeight);
    assert_eq!(first(&import_idml(&export_idml(&d)).unwrap()), FirstBaseline::EmboxHeight);
    let unknown = inset_fixture(r#"<TextFramePreference FirstBaselineOffset="SomethingElse"/>"#, "");
    assert_eq!(first(&unknown), FirstBaseline::Ascent);
}

#[test]
fn imports_and_round_trips_text_wrap_side_and_contour_type() {
    let wrap = r#"<TextWrapPreference TextWrapMode="Contour" TextWrapSide="LeftSide"><ContourOption ContourType="BoundingBox" IncludeInsideEdges="false"/></TextWrapPreference><Image Self="im""#;
    let spread = SPREAD.replace(r#"<Image Self="im""#, wrap);
    assert_ne!(spread, SPREAD);
    let bytes = zip_files(&[
        ("designmap.xml", DESIGNMAP),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Styles.xml", STYLES),
        ("Resources/Preferences.xml", PREFS),
        ("MasterSpreads/MasterSpread_m1.xml", MASTER),
        ("Spreads/Spread_sp1.xml", &spread),
        ("Stories/Story_s1.xml", STORY),
    ]);
    let mut d = import_idml(&bytes).unwrap();
    let id = d.spreads[0].items.iter().find(|i| i.graphic().is_some()).unwrap().id;
    let w = d.item(id).unwrap().wrap.clone();
    assert_eq!(
        (w.mode, w.side, w.contour),
        (designcraft_doc::WrapMode::Contour, designcraft_doc::WrapSide::LeftSide, designcraft_doc::ContourType::BoundingBox)
    );
    // Every side and contour type survives export and import (detected edges included, which
    // composition follows as the frame's path).
    d.item_mut(id).unwrap().wrap.side = designcraft_doc::WrapSide::AwayFromSpine;
    d.item_mut(id).unwrap().wrap.contour = designcraft_doc::ContourType::DetectEdges;
    let back = import_idml(&export_idml(&d)).unwrap();
    let w = back.spreads[0].items.iter().find(|i| i.graphic().is_some()).unwrap().wrap.clone();
    assert_eq!((w.side, w.contour), (designcraft_doc::WrapSide::AwayFromSpine, designcraft_doc::ContourType::DetectEdges));
}

fn feather_fixture(transparency: &str) -> Document {
    let designmap = r#"<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" Self="d">
          <idPkg:Spread src="Spreads/Spread_s.xml"/>
        </Document>"#;
    let rect = |id: &str, inner: &str| {
        format!(
            r#"<Rectangle Self="{id}" ItemTransform="1 0 0 1 0 0">
              <Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
                <PathPointType Anchor="0 0"/><PathPointType Anchor="0 100"/>
                <PathPointType Anchor="100 100"/><PathPointType Anchor="100 0"/>
              </PathPointArray></GeometryPathType></PathGeometry></Properties>
              {inner}
            </Rectangle>"#
        )
    };
    let spread = format!(
        r#"<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
          <Spread Self="s">
            <Page Self="p" GeometricBounds="0 0 200 200" ItemTransform="1 0 0 1 0 0"/>
            {}{}
          </Spread>
        </idPkg:Spread>"#,
        rect("a", transparency),
        rect("b", "")
    );
    import_idml(&zip_files(&[("designmap.xml", designmap), ("Spreads/Spread_s.xml", &spread)])).unwrap()
}

#[test]
fn imports_gradient_and_directional_feathers() {
    let d = feather_fixture(
        r#"<TransparencySetting>
             <GradientFeatherSetting Applied="true" Type="Linear" Angle="0" Length="50" GradientStart="10 20">
               <OpacityGradientStop Self="s0" Opacity="90" Location="20"/>
               <OpacityGradientStop Self="s2" Opacity="10" Location="80" Midpoint="40"/>
               <OpacityGradientStop Self="s1" Opacity="55" Location="50" Midpoint="50"/>
             </GradientFeatherSetting>
             <DirectionalFeatherSetting Applied="true" LeftWidth="3" RightWidth="4" TopWidth="5" BottomWidth="6" Angle="0" Noise="0" ChokeAmount="0"/>
           </TransparencySetting>"#,
    );
    let items = &d.spreads[0].items;
    let gf = &items[0].effects.gradient_feather;
    assert!(gf.on && !gf.radial);
    assert!((gf.start - 0.9).abs() < 1e-6 && (gf.end - 0.1).abs() < 1e-6, "{gf:?}");
    let v = gf.vector.unwrap();
    for (a, b) in v.iter().zip([20.0, 20.0, 50.0, 20.0]) {
        assert!((a - b).abs() < 1e-9, "{v:?}");
    }
    let df = &items[0].effects.directional_feather;
    assert!(df.on);
    assert_eq!(df.widths, [5.0, 3.0, 6.0, 4.0]);
    assert_eq!(items[1].effects, designcraft_doc::Effects::default());

    // Not applied: the settings stay off.
    let off = feather_fixture(
        r#"<TransparencySetting>
             <GradientFeatherSetting Applied="false" Length="50" GradientStart="10 20"/>
             <DirectionalFeatherSetting Applied="false" LeftWidth="3"/>
           </TransparencySetting>"#,
    );
    assert_eq!(off.spreads[0].items[0].effects, designcraft_doc::Effects::default());
}

#[test]
fn gradient_and_directional_feathers_round_trip() {
    use designcraft_doc::{DirectionalFeather, GradientFeather, Item, ItemId};
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let add = |d: &mut Document, effects: designcraft_doc::Effects| {
        let mut it = Item::new(ItemId(d.alloc()), lid, Shape::Rectangle, shapes::rectangle(Rect::new(40.0, 40.0, 140.0, 120.0)));
        it.effects = effects;
        d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    };
    let linear = designcraft_doc::Effects {
        gradient_feather: GradientFeather { on: true, radial: false, angle: 0.0, start: 0.75, end: 0.25, vector: Some([50.0, 60.0, 110.0, 60.0]) },
        directional_feather: DirectionalFeather { on: true, widths: [1.0, 2.0, 3.0, 4.0] },
        ..Default::default()
    };
    let radial = designcraft_doc::Effects {
        gradient_feather: GradientFeather { on: true, radial: true, angle: 30.0, start: 0.0, end: 1.0, vector: None },
        ..Default::default()
    };
    add(&mut d, linear.clone());
    add(&mut d, radial.clone());
    add(&mut d, designcraft_doc::Effects::default());
    let back = import_idml(&export_idml(&d)).unwrap();
    let effects: Vec<_> = back.spreads[0].items.iter().map(|i| i.effects.clone()).collect();
    assert_eq!(effects, [linear, radial, designcraft_doc::Effects::default()]);
}

/// One single-column text frame at (0, 0)-(300, 200) on page 1, beside a 100 pt wide rectangle
/// with a bounding-box wrap and `wrap_props` inside its `TextWrapPreference/Properties`.
fn custom_wrap_fixture(wrap_props: &str) -> Vec<u8> {
    let rect = |w: f64, h: f64| {
        format!(
            r#"<PathGeometry><GeometryPathType PathOpen="false"><PathPointArray><PathPointType Anchor="0 0"/><PathPointType Anchor="0 {h}"/><PathPointType Anchor="{w} {h}"/><PathPointType Anchor="{w} 0"/></PathPointArray></GeometryPathType></PathGeometry>"#
        )
    };
    let spread = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <Spread Self="sp1" PageCount="2" ItemTransform="1 0 0 1 0 0">
    <Page Self="p1" AppliedMaster="m1" GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 -500 -350"/>
    <Page Self="p2" AppliedMaster="m1" GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 0 -350"/>
    <TextFrame Self="t1" ParentStory="s1" PreviousTextFrame="n" NextTextFrame="n" ItemLayer="L1" ItemTransform="1 0 0 1 -450 -300">
      <Properties>{frame}</Properties>
    </TextFrame>
    <Rectangle Self="w" ItemLayer="L1" ItemTransform="1 0 0 1 -450 -300">
      <Properties>{obj}</Properties>
      <TextWrapPreference TextWrapMode="BoundingBoxTextWrap" TextWrapSide="BothSides"><Properties>{wrap_props}<TextWrapOffset Top="0" Left="0" Bottom="0" Right="0"/></Properties></TextWrapPreference>
    </Rectangle>
  </Spread>
</idPkg:Spread>"#,
        frame = rect(300.0, 200.0),
        obj = rect(100.0, 200.0),
    );
    let words = "wrap ".repeat(120);
    let story = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <Story Self="s1"><ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/NormalParagraphStyle"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>{words}</Content></CharacterStyleRange></ParagraphStyleRange></Story>
</idPkg:Story>"#
    );
    zip_files(&[
        ("designmap.xml", DESIGNMAP),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Styles.xml", STYLES),
        ("Resources/Preferences.xml", PREFS),
        ("MasterSpreads/MasterSpread_m1.xml", MASTER),
        ("Spreads/Spread_sp1.xml", &spread),
        ("Stories/Story_s1.xml", &story),
    ])
}

/// The custom boundary: 40 pt wide, the full height of the 100 pt wide rectangle.
const NARROW_WRAP_PATH: &str = r#"<PathGeometry><GeometryPathType PathOpen="false"><PathPointArray><PathPointType Anchor="0 0" LeftDirection="0 0" RightDirection="0 0"/><PathPointType Anchor="0 200" LeftDirection="0 200" RightDirection="0 200"/><PathPointType Anchor="40 200" LeftDirection="40 200" RightDirection="40 200"/><PathPointType Anchor="40 0" LeftDirection="40 0" RightDirection="40 0"/></PathPointArray></GeometryPathType></PathGeometry>"#;

fn wrapping_item(d: &Document) -> &designcraft_doc::Item {
    d.spreads[0].items.iter().find(|i| i.wrap.mode != designcraft_doc::WrapMode::None).unwrap()
}

/// Leftmost line start in the frame.
fn first_line_x(d: &Document) -> f64 {
    let sid = *d.stories.keys().next().unwrap();
    let cs = designcraft_compose::compose_story(d, sid, &Default::default());
    assert!(cs.frames[0].lines.len() > 3);
    cs.frames[0].lines.iter().map(|l| l.x0).fold(f64::INFINITY, f64::min)
}

#[test]
fn custom_wrap_boundary_imports_composes_and_round_trips() {
    let plain = import_idml_with(&custom_wrap_fixture(""), &|_| None).unwrap();
    assert_eq!(wrapping_item(&plain).wrap.path, None);
    assert!(first_line_x(&plain) >= 100.0 - 1e-6, "the frame's bounds push text to x = 100");

    let d = import_idml_with(&custom_wrap_fixture(NARROW_WRAP_PATH), &|_| None).unwrap();
    let path = wrapping_item(&d).wrap.path.clone().unwrap();
    assert_eq!(path.bounds(), Some(Rect::new(0.0, 0.0, 40.0, 200.0)));
    let x = first_line_x(&d);
    assert!((40.0 - 1e-6..100.0).contains(&x), "lines start at the custom boundary's edge, not the frame's: {x}");

    let back = import_idml(&export_idml(&d)).unwrap();
    assert_eq!(wrapping_item(&back).wrap.path, Some(path));
}

#[test]
fn hostile_custom_wrap_boundary_is_dropped() {
    let nan = NARROW_WRAP_PATH.replacen(r#"Anchor="40 200""#, r#"Anchor="NaN 200""#, 1);
    let inf = NARROW_WRAP_PATH.replacen(r#"Anchor="40 200""#, r#"Anchor="inf 200""#, 1);
    let huge = format!(
        r#"<PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>{}</PathPointArray></GeometryPathType></PathGeometry>"#,
        r#"<PathPointType Anchor="1 1"/>"#.repeat(50_000)
    );
    let empty = r#"<PathGeometry><GeometryPathType PathOpen="false"><PathPointArray/></GeometryPathType></PathGeometry>"#;
    for props in [nan.as_str(), inf.as_str(), huge.as_str(), empty] {
        let d = import_idml_with(&custom_wrap_fixture(props), &|_| None).unwrap();
        let it = wrapping_item(&d);
        assert_eq!(it.wrap.mode, designcraft_doc::WrapMode::BoundingBox);
        assert_eq!(it.wrap.path, None);
        assert!(first_line_x(&d) >= 100.0 - 1e-6);
    }
}

/// A family name keeps its spaces ("Minion Pro " is a font of its own) and `FontStyle="Nothing"`
/// leaves the style to the paragraph, also after an export.
#[test]
fn applied_font_keeps_spaces_and_font_style_nothing_inherits() {
    let story = r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Story Self="s1">
        <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/Text%3aBody" FontStyle="Bold">
        <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" FontStyle="Nothing"><Properties><AppliedFont type="string">DC Space </AppliedFont></Properties><Content>Spaced</Content></CharacterStyleRange>
        <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" FontStyle="$ID/Nothing"><Content> plain</Content></CharacterStyleRange>
        </ParagraphStyleRange></Story></idPkg:Story>"#;
    let check = |d: &Document| {
        let s = d.stories.values().find(|s| s.text.contains("Spaced")).unwrap();
        let (_, base) = d.styles.resolve_para(&s.paras[0]);
        let runs: Vec<_> = s.runs().map(|(_, f)| f.clone()).collect();
        assert_eq!(runs[0].over.font_family.as_deref(), Some("DC Space "));
        for f in &runs {
            assert_eq!(d.styles.resolve_char(&base, f).font_style, "Bold", "the enclosing range's style");
        }
    };
    let d = import_idml(&fixture_with_story(story)).unwrap();
    check(&d);
    check(&import_idml(&export_idml(&d)).unwrap());
}

fn tracked_story(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <Story Self="s1">{body}</Story>
</idPkg:Story>"#
    )
}

const CSR: &str = r#"<CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]">"#;

#[test]
fn tracked_deleted_text_takes_no_room_and_round_trips() {
    let psr = r#"<ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/NormalParagraphStyle">"#;
    let kwn = r#"<ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/NormalParagraphStyle" KeepWithNext="2">"#;
    let tracked = tracked_story(&format!(
        r#"{psr}{CSR}<Content>The first paragraph keeps a </Content><Change Self="c1" ChangeType="DeletedText"><Content>deleted </Content></Change><Content>word.</Content><Br/></CharacterStyleRange></ParagraphStyleRange>
{kwn}{CSR}<Change Self="c2" ChangeType="DeletedText"><Br/></Change></CharacterStyleRange></ParagraphStyleRange>
{psr}{CSR}<Change Self="c3" ChangeType="InsertedText"><Content>New </Content></Change><Content>second paragraph.</Content></CharacterStyleRange></ParagraphStyleRange>"#
    ));
    let plain = tracked_story(&format!(
        r#"{psr}{CSR}<Content>The first paragraph keeps a word.</Content><Br/></CharacterStyleRange></ParagraphStyleRange>
{psr}{CSR}<Content>New second paragraph.</Content></CharacterStyleRange></ParagraphStyleRange>"#
    ));
    let lines = |d: &Document| {
        let sid = *d.stories.keys().next().unwrap();
        let cs = designcraft_compose::compose_story(d, sid, &Default::default());
        cs.frames.iter().flat_map(|f| f.lines.iter().map(|l| (l.baseline, l.x0, l.end_x))).collect::<Vec<_>>()
    };
    let d = import_idml_with(&fixture_with_story(&tracked), &|_| None).unwrap();
    let story = d.stories.values().next().unwrap();
    assert_eq!(story.text, "The first paragraph keeps a deleted word.\n\nNew second paragraph.");
    let marks = |s: &designcraft_doc::Story| s.runs().map(|(r, f)| (s.text[r].to_string(), f.over.change)).collect::<Vec<_>>();
    use designcraft_doc::ChangeMark::{Deleted, Inserted};
    let want = vec![
        ("The first paragraph keeps a ".to_string(), None),
        ("deleted ".to_string(), Some(Deleted)),
        ("word.\n".to_string(), None),
        ("\n".to_string(), Some(Deleted)),
        ("New ".to_string(), Some(Inserted)),
        ("second paragraph.".to_string(), None),
    ];
    assert_eq!(marks(story), want);
    let reference = import_idml_with(&fixture_with_story(&plain), &|_| None).unwrap();
    let expected = lines(&reference);
    assert!(expected.len() >= 2);
    assert_eq!(lines(&d), expected);
    // Export writes the changes back as `Change` elements.
    let back = import_idml(&export_idml(&d)).unwrap();
    assert_eq!(marks(back.stories.values().next().unwrap()), want);
}

#[test]
fn same_para_style_spacing_imports_and_round_trips() {
    let story = r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Story Self="s1">
    <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/[No paragraph style]" SameParaStyleSpacing="0">
    <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>Gap</Content><Br/></CharacterStyleRange>
    </ParagraphStyleRange>
    <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/[No paragraph style]" SameParaStyleSpacing="Nothing">
    <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>Ignore</Content></CharacterStyleRange>
    </ParagraphStyleRange></Story></idPkg:Story>"#;
    let d = import_idml(&fixture_with_story(story)).unwrap();
    let st = d.stories.values().find(|s| s.text.contains("Gap")).unwrap();
    assert_eq!(st.paras[0].para.same_para_style_spacing, Some(Some(0.0)));
    // Ignore is an explicit "off", not an unset value.
    assert_eq!(st.paras[1].para.same_para_style_spacing, Some(None));
    let back = import_idml(&export_idml(&d)).unwrap();
    let b = back.stories.values().find(|s| s.text.contains("Gap")).unwrap();
    assert_eq!(b.paras[0].para.same_para_style_spacing, Some(Some(0.0)));
    // The explicit Ignore survives the round trip too.
    assert_eq!(b.paras[1].para.same_para_style_spacing, Some(None));
}

/// SameParaStyleSpacing set as a `Properties` element on the based-on style (`Normal`) reaches
/// the headings: consecutive headings sit one leading apart. SetIgnore keeps the usual spacing.
#[test]
fn same_para_style_spacing_from_a_based_on_style_element_closes_the_gap() {
    let styles = r#"<?xml version="1.0" encoding="UTF-8"?>
<idPkg:Styles xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <RootCharacterStyleGroup Self="rc">
    <CharacterStyle Self="CharacterStyle/$ID/[No character style]" Name="$ID/[No character style]"/>
  </RootCharacterStyleGroup>
  <RootParagraphStyleGroup Self="rp">
    <ParagraphStyle Self="ParagraphStyle/$ID/[No paragraph style]" Name="$ID/[No paragraph style]" PointSize="12">
      <Properties><AppliedFont type="string">Source Serif 4</AppliedFont><Leading type="unit">13</Leading></Properties>
    </ParagraphStyle>
    <ParagraphStyle Self="ParagraphStyle/$ID/NormalParagraphStyle" Name="$ID/NormalParagraphStyle">
      <Properties><BasedOn type="string">$ID/[No paragraph style]</BasedOn><SameParaStyleSpacing type="unit">0</SameParaStyleSpacing></Properties>
    </ParagraphStyle>
    <ParagraphStyle Self="ParagraphStyle/Heading" Name="Heading" SpaceBefore="18" SpaceAfter="4.5">
      <Properties><BasedOn type="object">ParagraphStyle/$ID/NormalParagraphStyle</BasedOn></Properties>
    </ParagraphStyle>
    <ParagraphStyle Self="ParagraphStyle/Loose" Name="Loose" SpaceBefore="18" SpaceAfter="4.5">
      <Properties><BasedOn type="object">ParagraphStyle/$ID/NormalParagraphStyle</BasedOn><SameParaStyleSpacing type="enumeration">SetIgnore</SameParaStyleSpacing></Properties>
    </ParagraphStyle>
  </RootParagraphStyleGroup>
</idPkg:Styles>"#;
    let para = |style: &str, text: &str| {
        format!(
            r#"<ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/{style}"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>{text}</Content><Br/></CharacterStyleRange></ParagraphStyleRange>"#
        )
    };
    let story = format!(
        r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Story Self="s1">{}{}{}{}</Story></idPkg:Story>"#,
        para("Heading", "One"),
        para("Heading", "Two"),
        para("Loose", "Three"),
        para("Loose", "Four")
    );
    let d = import_idml(&fixture_with_table_styles(styles, &story)).unwrap();
    let sid = d.stories.iter().find(|(_, s)| s.text.contains("One")).map(|(id, _)| *id).unwrap();
    let cs = designcraft_compose::compose_story(&d, sid, &Default::default());
    let baseline = |para: usize| cs.frames[0].lines.iter().find(|l| l.para == para).unwrap().baseline;
    assert!((baseline(1) - baseline(0) - 13.0).abs() < 1e-6, "{} {}", baseline(0), baseline(1));
    assert!((baseline(3) - baseline(2) - 13.0 - 4.5 - 18.0).abs() < 1e-6, "{} {}", baseline(2), baseline(3));
}

/// Composed lines (baseline, left, right) of the only story.
fn story_lines(d: &Document) -> Vec<(f64, f64, f64)> {
    let sid = *d.stories.keys().next().unwrap();
    let cs = designcraft_compose::compose_story(d, sid, &Default::default());
    cs.frames.iter().flat_map(|f| f.lines.iter().map(|l| (l.baseline, l.x0, l.end_x))).collect()
}

const PSR: &str = r#"<ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/NormalParagraphStyle">"#;

/// Lays out like the same story without the deleted paragraph.
fn assert_deleted_para_takes_no_room(deleted: &str, deleted_text: &str) {
    let tracked = tracked_story(&format!(
        r#"{PSR}{CSR}<Content>First live paragraph.</Content><Br/></CharacterStyleRange></ParagraphStyleRange>
{deleted}
{PSR}{CSR}<Content>Second live paragraph.</Content></CharacterStyleRange></ParagraphStyleRange>"#
    ));
    let plain = tracked_story(&format!(
        r#"{PSR}{CSR}<Content>First live paragraph.</Content><Br/></CharacterStyleRange></ParagraphStyleRange>
{PSR}{CSR}<Content>Second live paragraph.</Content></CharacterStyleRange></ParagraphStyleRange>"#
    ));
    let d = import_idml_with(&fixture_with_story(&tracked), &|_| None).unwrap();
    let story = d.stories.values().next().unwrap();
    let marks: Vec<_> = story.runs().map(|(r, f)| (story.text[r].to_string(), f.over.change)).collect();
    let want = vec![
        ("First live paragraph.\n".to_string(), None),
        (deleted_text.to_string(), Some(designcraft_doc::ChangeMark::Deleted)),
        ("Second live paragraph.".to_string(), None),
    ];
    assert_eq!(marks, want);
    let expected = story_lines(&import_idml_with(&fixture_with_story(&plain), &|_| None).unwrap());
    assert_eq!(expected.len(), 2);
    assert_eq!(story_lines(&d), expected);
}

#[test]
fn tracked_deleted_paragraph_range_stays_deleted() {
    assert_deleted_para_takes_no_room(
        &format!(
            r#"<Change Self="c1" ChangeType="DeletedText">{PSR}{CSR}<Content>Deleted text</Content><Br/></CharacterStyleRange></ParagraphStyleRange></Change>"#
        ),
        "Deleted text\n",
    );
}

#[test]
fn tracked_deleted_frame_break_does_not_break() {
    let fb = "\u{E003}";
    let nf = r#"<CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" ParagraphBreakType="NextFrame">"#;
    // The change around the paragraph range.
    assert_deleted_para_takes_no_room(
        &format!(
            r#"<Change Self="c1" ChangeType="DeletedText">{PSR}{CSR}<Content>Gone</Content></CharacterStyleRange>{nf}<Br/></CharacterStyleRange></ParagraphStyleRange></Change>"#
        ),
        &format!("Gone{fb}\n"),
    );
    // The change inside the character range.
    assert_deleted_para_takes_no_room(
        &format!(
            r#"{PSR}{nf}<Change Self="c1" ChangeType="DeletedText"><Content>Gone</Content><Br/></Change></CharacterStyleRange></ParagraphStyleRange>"#
        ),
        &format!("Gone{fb}\n"),
    );
}

#[test]
fn live_frame_break_before_deleted_paragraph_still_breaks() {
    let nf = r#"<CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" ParagraphBreakType="NextFrame">"#;
    let first =
        format!(r#"{PSR}{CSR}<Content>First live paragraph.</Content></CharacterStyleRange>{nf}<Br/></CharacterStyleRange></ParagraphStyleRange>"#);
    let second = format!(r#"{PSR}{CSR}<Content>Second live paragraph.</Content></CharacterStyleRange></ParagraphStyleRange>"#);
    let deleted = format!(
        r#"<Change Self="c1" ChangeType="DeletedText">{PSR}{CSR}<Content>Gone</Content><Br/></CharacterStyleRange></ParagraphStyleRange></Change>"#
    );
    let import = |body: String| import_idml_with(&fixture_with_story(&tracked_story(&body)), &|_| None).unwrap();
    let reference = import(format!("{first}\n{second}"));
    let sid = *reference.stories.keys().next().unwrap();
    let per_frame: Vec<usize> =
        designcraft_compose::compose_story(&reference, sid, &Default::default()).frames.iter().map(|f| f.lines.len()).collect();
    assert!(per_frame.first() == Some(&1) && per_frame.get(1).is_some_and(|n| *n > 0), "the break sends the second paragraph on");
    let expected = story_lines(&reference);
    assert_eq!(story_lines(&import(format!("{first}\n{deleted}\n{second}"))), expected);
}

#[test]
fn vertical_balance_columns_balances_the_imported_frame() {
    let text = "Balanced columns end at about the same depth when the story ends in the frame. ".repeat(4);
    let designmap = r#"<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" Self="d">
          <idPkg:Spread src="Spreads/Spread_s.xml"/>
          <idPkg:Story src="Stories/Story_s1.xml"/>
        </Document>"#;
    let spread = r#"<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
          <Spread Self="s">
            <Page Self="p" GeometricBounds="0 0 400 400" ItemTransform="1 0 0 1 0 0"/>
            <TextFrame Self="f" ParentStory="s1">
              <Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
                <PathPointType Anchor="0 0"/><PathPointType Anchor="0 150"/>
                <PathPointType Anchor="300 150"/><PathPointType Anchor="300 0"/>
              </PathPointArray></GeometryPathType></PathGeometry></Properties>
              <TextFramePreference TextColumnCount="2" TextColumnGutter="12" VerticalBalanceColumns="true"/>
            </TextFrame>
          </Spread>
        </idPkg:Spread>"#;
    let story = format!(
        r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Story Self="s1">
          <ParagraphStyleRange><CharacterStyleRange><Content>{text}</Content></CharacterStyleRange></ParagraphStyleRange>
        </Story></idPkg:Story>"#
    );
    let mut d = import_idml(&zip_files(&[("designmap.xml", designmap), ("Spreads/Spread_s.xml", spread), ("Stories/Story_s1.xml", &story)])).unwrap();
    let sid = *d.stories.keys().next().unwrap();
    let per_column = |d: &Document| {
        let cs = designcraft_compose::compose_story(d, sid, &Default::default());
        assert!(!cs.is_overset());
        let ft = &cs.frames[0];
        [0, 1].map(|c| ft.lines.iter().filter(|l| l.column == c).count())
    };
    let on = per_column(&d);
    assert!(on[1] > 0 && on[0] - on[1] <= 1, "balanced: {on:?}");
    let fid = d.stories[&sid].frames[0];
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.balance_columns = false;
    let off = per_column(&d);
    assert!(off[0] > off[1] + 1, "unbalanced: {off:?}");
}

#[test]
fn type_on_a_path_keeps_its_start_and_end_brackets() {
    use designcraft_geom::{Anchor, PathData, Point, SubPath};
    for end in [Some(250.0), None] {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let id = designcraft_doc::ItemId(d.alloc());
        let line = PathData::single(SubPath::new(vec![Anchor::corner(Point::new(100.0, 300.0)), Anchor::corner(Point::new(500.0, 300.0))], false));
        let mut it = designcraft_doc::Item::new(id, lid, Shape::GraphicLine, line);
        let sid = designcraft_doc::StoryId(d.alloc());
        let mut tf = designcraft_doc::TextFrame { story: sid, options: Default::default() };
        tf.options.path = Some(designcraft_doc::PathType { start: 30.0, end, ..Default::default() });
        it.content = designcraft_doc::Content::Text(tf);
        let mut story = designcraft_doc::Story::with_text(sid, "Along", Default::default());
        story.frames = vec![id];
        d.stories.insert(sid, std::sync::Arc::new(story));
        d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
        let back = import_idml(&export_idml(&d)).unwrap();
        let pt = back.spreads[0].items[0].text_frame().unwrap().options.path.clone().unwrap();
        assert!((pt.start - 30.0).abs() < 1e-6, "{pt:?}");
        assert_eq!(pt.end.map(|e| e.round()), end, "an end bracket at the path's end is no end bracket");
    }
}

/// Based on Parent travels as the parent pages' `AppliedMaster`; a cycle in a file is broken on
/// import with a warning.
#[test]
fn parent_based_on_round_trips_and_cycles_are_broken() {
    let mut d = Document::new(&NewDocument { pages: 2, ..Default::default() });
    let a = d.parents[0].id;
    let b = d.add_parent("B", "Opening", 2, 12.0, designcraft_doc::Margins::uniform(36.0));
    d.add_parent("C", "Closing", 2, 12.0, designcraft_doc::Margins::uniform(36.0));
    let set = |d: &mut Document, pi: usize, on| std::sync::Arc::make_mut(&mut d.parents[pi]).parent.as_mut().unwrap().based_on = on;
    set(&mut d, 1, Some(a));
    set(&mut d, 2, Some(b));
    let back = import_idml(&export_idml(&d)).unwrap();
    let base_of = |d: &Document, pi: usize| {
        let b = d.parents[pi].parent.as_ref().unwrap().based_on?;
        d.parents.iter().find(|p| p.id == b).and_then(|p| p.parent.as_ref()).map(|i| i.prefix.clone())
    };
    assert_eq!((base_of(&back, 0), base_of(&back, 1), base_of(&back, 2)), (None, Some("A".into()), Some("B".into())));

    set(&mut d, 0, Some(b));
    let r = import_idml_report(&export_idml(&d), &|_| None).unwrap();
    assert!(r.warnings.iter().any(|w| w.contains("A-Parent") && w.contains("cycle")), "{:?}", r.warnings);
    assert_eq!((base_of(&r.document, 0), base_of(&r.document, 1)), (None, Some("A".into())));
}

#[test]
fn object_style_effects_round_trip() {
    use designcraft_doc::{DropShadow, Effects, ObjectStyle};
    let mut d = Document::new(&NewDocument::default());
    let fx = Effects {
        drop_shadow: DropShadow { on: true, distance: 5.0, size: 12.0, opacity: 0.28, ..Default::default() },
        feather: 3.0,
        ..Default::default()
    };
    d.styles_mut().object.push(ObjectStyle { name: "Photo".into(), effects: Some(fx.clone()), ..Default::default() });
    d.styles_mut().object.push(ObjectStyle { name: "Plain".into(), fill: Some(Fill::swatch("Black")), ..Default::default() });
    let lid = d.default_layer();
    let id = designcraft_doc::ItemId(d.alloc());
    let mut it = designcraft_doc::Item::new(id, lid, Shape::Rectangle, shapes::rectangle(Rect::new(40.0, 40.0, 140.0, 140.0)));
    it.object_style = "Photo".into();
    it.effects = fx.clone();
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    // A frame that turned the style's effects off writes them off, so it doesn't take the style's.
    let id = designcraft_doc::ItemId(d.alloc());
    let mut off = designcraft_doc::Item::new(id, lid, Shape::Rectangle, shapes::rectangle(Rect::new(200.0, 40.0, 300.0, 140.0)));
    off.object_style = "Photo".into();
    d.insert_item(SpreadRef::Doc(0), off, None).unwrap();

    let bytes = export_idml(&d);
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.as_slice())).unwrap();
    let mut spreads = String::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        if f.name().starts_with("Spreads/") {
            std::io::Read::read_to_string(&mut f, &mut spreads).unwrap();
        }
    }
    assert!(spreads.contains(r#"<DropShadowSetting Mode="None""#) && spreads.contains(r#"<FeatherSetting Mode="None""#), "{spreads}");
    let back = import_idml(&bytes).unwrap();
    let style = |n: &str| back.styles.object.iter().find(|s| s.name == n).unwrap();
    let got = style("Photo").effects.clone().unwrap();
    let ds = &got.drop_shadow;
    assert!(ds.on && ds.size == 12.0 && (ds.opacity - 0.28).abs() < 1e-6, "{ds:?}");
    assert!((ds.distance - 5.0).abs() < 1e-6 && (ds.angle - DropShadow::default().angle).abs() < 1e-6, "{ds:?}");
    assert_eq!(got.feather, 3.0);
    assert!(style("Plain").effects.is_none());
    // The frame's effects are the style's, so it follows later edits of the style.
    let frames: Vec<_> = back.spreads[0].items.iter().filter(|i| i.object_style == "Photo").collect();
    assert_eq!(frames[0].effects, got);
    assert_eq!(frames[1].effects, Effects::default());
}

#[test]
fn object_style_effects_import_by_category_and_based_on() {
    let styles = r#"
      <ObjectStyle Self="ObjectStyle/Base" Name="Base">
        <TransparencySetting>
          <DropShadowSetting Mode="Drop" XOffset="3" YOffset="4" Size="9" Opacity="50"/>
          <FeatherSetting Mode="Standard" Width="6"/>
        </TransparencySetting>
        <ObjectStyleObjectEffectsCategorySettings EnableDropShadow="true" EnableFeather="false"/>
      </ObjectStyle>
      <ObjectStyle Self="ObjectStyle/Child" Name="Child">
        <Properties><BasedOn type="object">ObjectStyle/Base</BasedOn></Properties>
        <ObjectStyleObjectEffectsCategorySettings EnableDropShadow="true" EnableFeather="true"/>
      </ObjectStyle>
      <ObjectStyle Self="ObjectStyle/Off" Name="Off">
        <TransparencySetting><DropShadowSetting Mode="Drop" Size="9"/></TransparencySetting>
        <ObjectStyleObjectEffectsCategorySettings EnableDropShadow="false" EnableFeather="false"/>
      </ObjectStyle>
      <ObjectStyle Self="ObjectStyle/Loop" Name="Loop">
        <Properties><BasedOn type="object">ObjectStyle/Loop</BasedOn></Properties>
        <ObjectStyleObjectEffectsCategorySettings EnableDropShadow="true"/>
      </ObjectStyle>"#;
    let d = inset_fixture("", styles);
    let fx = |n: &str| d.styles.object.iter().find(|s| s.name == n).unwrap().effects.clone();
    let base = fx("Base").unwrap();
    assert!(base.drop_shadow.on && base.drop_shadow.size == 9.0 && (base.drop_shadow.distance - 5.0).abs() < 1e-9);
    assert_eq!(base.feather, 0.0, "EnableFeather=false");
    let child = fx("Child").unwrap();
    assert_eq!((child.drop_shadow.size, child.feather), (9.0, 6.0), "settings from the BasedOn style");
    assert!(fx("Off").is_none());
    assert!(!fx("Loop").unwrap().drop_shadow.on);
}

#[test]
fn a_strokes_gradient_vector_round_trips() {
    let mut d = designcraft_doc::Document::new(&designcraft_doc::build::NewDocument::default());
    let id = designcraft_doc::ItemId(d.alloc());
    let mut it = designcraft_doc::Item::new(
        id,
        d.default_layer(),
        designcraft_doc::Shape::Path,
        designcraft_geom::shapes::rectangle(designcraft_geom::Rect::new(10.0, 10.0, 110.0, 60.0)),
    );
    it.stroke.gradient_vector = Some([10.0, 35.0, 110.0, 35.0]);
    d.insert_item(designcraft_doc::SpreadRef::Doc(0), it, None).unwrap();
    let back = crate::import_idml(&crate::export_idml(&d)).unwrap();
    let v = back.spreads[0].items[0].stroke.gradient_vector.unwrap();
    assert!(v.iter().zip([10.0, 35.0, 110.0, 35.0]).all(|(a, b)| (a - b).abs() < 1e-6), "{v:?}");
}

#[test]
fn feather_choke_and_corners_round_trip() {
    use designcraft_doc::{Effects, FeatherCorners, FeatherOptions, Item, ItemId, ObjectStyle};
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let feathers = [(6.0, 40.0, FeatherCorners::Sharp), (4.0, 0.0, FeatherCorners::Rounded), (3.0, 12.5, FeatherCorners::Diffused)];
    let fx = |(w, choke, corners): (f64, f64, FeatherCorners)| Effects {
        feather: w,
        feather_options: FeatherOptions { choke, corners },
        ..Default::default()
    };
    for (i, f) in feathers.into_iter().enumerate() {
        let x = 40.0 + 120.0 * i as f64;
        let mut it = Item::new(ItemId(d.alloc()), lid, Shape::Rectangle, shapes::rectangle(Rect::new(x, 40.0, x + 100.0, 140.0)));
        it.effects = fx(f);
        d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    }
    let styled = fx((5.0, 25.0, FeatherCorners::Rounded));
    d.styles_mut().object.push(ObjectStyle { name: "Soft".into(), effects: Some(styled.clone()), ..Default::default() });
    let back = import_idml(&export_idml(&d)).unwrap();
    let got: Vec<_> = back.spreads[0].items.iter().map(|i| i.effects.clone()).collect();
    assert_eq!(got, feathers.map(fx));
    let style = back.styles.object.iter().find(|s| s.name == "Soft").and_then(|s| s.effects.clone()).unwrap();
    assert_eq!((style.feather, style.feather_options), (styled.feather, styled.feather_options));
}

#[test]
fn style_names_ending_in_a_space_and_partial_rule_overrides_import() {
    // Nested style "Decimals " (a name with a trailing space, as InDesign allows) and a label
    // style that only recolours the rule of the style it is based on.
    let styles = STYLES
        .replace(
            r#"  </RootCharacterStyleGroup>"#,
            r#"    <CharacterStyle Self="CharacterStyle/Decimals " Name="Decimals " Position="Superscript"/>
  </RootCharacterStyleGroup>"#,
        )
        .replace(
            r#"  </RootParagraphStyleGroup>"#,
            r#"    <ParagraphStyle Self="ParagraphStyle/Price" Name="Price">
      <Properties><BasedOn type="string">$ID/[No paragraph style]</BasedOn>
        <AllNestedStyles type="list"><ListItem type="record"><AppliedCharacterStyle type="object">CharacterStyle/Decimals </AppliedCharacterStyle><Delimiter type="enumeration">AnyCharacter</Delimiter><Repetition type="long">2</Repetition><Inclusive type="boolean">true</Inclusive></ListItem></AllNestedStyles>
      </Properties>
    </ParagraphStyle>
    <ParagraphStyle Self="ParagraphStyle/Label white" Name="Label white" RuleAbove="true" RuleAboveLineWeight="16" RuleAboveOffset="-3">
      <Properties><BasedOn type="string">$ID/[No paragraph style]</BasedOn><RuleAboveColor type="object">Color/Paper</RuleAboveColor></Properties>
    </ParagraphStyle>
    <ParagraphStyle Self="ParagraphStyle/Label black" Name="Label black">
      <Properties><BasedOn type="object">ParagraphStyle/Label white</BasedOn><RuleAboveColor type="object">Color/Black</RuleAboveColor></Properties>
    </ParagraphStyle>
  </RootParagraphStyleGroup>"#,
        );
    let d = import_idml(&fixture_with_table_styles(&styles, STORY)).unwrap();
    let style = |n: &str| d.styles.paragraph.iter().find(|s| s.name == n).unwrap();
    let nested = style("Price").para.nested_styles.clone().unwrap();
    assert_eq!(nested[0].style, "Decimals ", "the trailing space is part of the name");
    let white = style("Label white").para.rule_above.clone().unwrap();
    let black = style("Label black").para.rule_above.clone().unwrap();
    assert!(black.on, "the rule stays on");
    assert_eq!((black.weight, black.offset), (16.0, -3.0), "weight and offset come from the base style");
    assert_ne!(black.color, white.color, "only the colour is overridden");
}

#[test]
fn missing_linked_graphics_show_their_preview_and_keep_their_link() {
    // InDesign's preview of the missing photo (a PNG header is all the importer reads: 4 × 3).
    let png: Vec<u8> = [b"\x89PNG\r\n\x1a\n".as_slice(), &[0, 0, 0, 13], b"IHDR", &4u32.to_be_bytes(), &3u32.to_be_bytes()].concat();
    let previews = std::collections::HashMap::from([("im".to_string(), std::sync::Arc::new(png.clone()))]);
    let d = crate::import_idml_report_with_previews(&fixture(), &|_| None, &previews).unwrap().document;
    let img = d.spreads.iter().flat_map(|sp| sp.items.iter()).find(|i| i.graphic().is_some()).unwrap();
    let a = &d.assets[&img.graphic().unwrap().asset];
    assert_eq!(a.data.as_slice(), png.as_slice(), "the preview shows");
    assert_eq!(a.mime, "image/jpeg", "the link's format, not the preview's");
    assert_eq!(a.link.as_deref(), Some("/definitely/missing dir/photo.jpg"), "relinking still finds the file");
    // Without a preview it stays empty, as before.
    let d = import_idml(&fixture()).unwrap();
    assert!(d.assets.values().all(|a| a.data.is_empty() || a.link.is_none()));
}

#[test]
fn a_preview_keeps_the_links_format_and_pixel_size_and_is_shared() {
    let png: std::sync::Arc<Vec<u8>> =
        std::sync::Arc::new([b"\x89PNG\r\n\x1a\n".as_slice(), &[0, 0, 0, 13], b"IHDR", &4u32.to_be_bytes(), &3u32.to_be_bytes()].concat());
    let spread = SPREAD
        .replace(r#"<Image Self="im" ItemTransform="1 0 0 1 0 0">"#, r#"<Image Self="im" ItemTransform="1 0 0 1 0 0" ActualPpi="300 300">"#)
        .replace(
            "  </Spread>",
            r#"    <Rectangle Self="pf" ItemLayer="L1" ItemTransform="1 0 0 1 300 100" ContentType="GraphicType">
      <Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
        <PathPointType Anchor="0 0"/><PathPointType Anchor="0 30"/><PathPointType Anchor="40 30"/><PathPointType Anchor="40 0"/>
      </PathPointArray></GeometryPathType></PathGeometry></Properties>
      <PDF Self="pd" ItemTransform="1 0 0 1 0 0">
        <Properties><GraphicBounds Left="0" Top="0" Right="40" Bottom="30"/></Properties>
        <Link Self="lp" LinkResourceURI="file:/definitely/missing%20dir/logo.pdf" StoredState="Normal"/>
      </PDF>
    </Rectangle>
  </Spread>"#,
        );
    let idml = zip_files(&[
        ("designmap.xml", DESIGNMAP),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Styles.xml", STYLES),
        ("Resources/Preferences.xml", PREFS),
        ("MasterSpreads/MasterSpread_m1.xml", MASTER),
        ("Spreads/Spread_sp1.xml", &spread),
        ("Stories/Story_s1.xml", STORY),
    ]);
    // One preview for both graphics, as indd-utils groups them.
    let previews = std::collections::HashMap::from([("im".to_string(), png.clone()), ("pd".to_string(), png.clone())]);
    let d = crate::import_idml_report_with_previews(&idml, &|_| None, &previews).unwrap().document;
    let asset = |name: &str| d.assets.values().find(|a| a.name == name).unwrap();
    let (photo, logo) = (asset("photo.jpg"), asset("logo.pdf"));
    // 40 × 30 pt at 300 ppi, from the link, not the preview's 4 × 3.
    assert_eq!((photo.mime.as_str(), photo.pixels), ("image/jpeg", Some((167, 125))));
    assert_eq!(logo.mime, "application/pdf", "a placed PDF stays a PDF, for IDML export");
    assert!(std::sync::Arc::ptr_eq(&photo.data, &png) && std::sync::Arc::ptr_eq(&logo.data, &png), "the preview's bytes are not copied");
}

#[test]
fn frame_preference_import_caps_column_count_without_composition() {
    for (raw, expected) in [("4000000000", 216), ("1e300", 216), ("-100", 1), ("0", 1), ("NaN", 1), ("inf", 1), ("-inf", 1)] {
        for attribute in [true, false] {
            let preference = if attribute {
                format!(r#"<TextFramePreference TextColumnCount="{raw}"/>"#)
            } else {
                format!(
                    r#"<TextFramePreference><Properties><TextColumnCount type="number">{raw}</TextColumnCount></Properties></TextFramePreference>"#
                )
            };
            let document = format!(
                r#"<Document><Story Self="s"/><Spread Self="spread"><Page Self="page"/><TextFrame Self="frame" ParentStory="s">{preference}</TextFrame></Spread></Document>"#
            );
            let d = import_idml_with(&zip_files(&[("designmap.xml", &document)]), &|_| None).unwrap();
            let frame = d.spreads[0].items[0].text_frame().unwrap();
            // Inspect imported data only: never compose an unbounded input value.
            assert_eq!(frame.options.columns, expected, "{raw}, attribute={attribute}");
        }
    }
}

#[test]
fn frame_preference_property_color_resolves_for_frames_and_object_styles() {
    let document = r#"<Document>
<Color Self="Color/Near" Model="Process" Space="RGB" ColorValue="255 0 0" Name="Near" Visible="true"/>
<Color Self="Color/Far" Model="Process" Space="RGB" ColorValue="0 0 255" Name="Far" Visible="true"/>
<RootObjectStyleGroup Self="root">
<ObjectStyle Self="ObjectStyle/Base" Name="Base" EnableTextFrameGeneralOptions="true"><TextFramePreference ColumnRuleStrokeColor="Color/Far"/></ObjectStyle>
<ObjectStyle Self="ObjectStyle/Child" Name="Child" EnableTextFrameGeneralOptions="true"><Properties><BasedOn type="object">ObjectStyle/Base</BasedOn></Properties><TextFramePreference><Properties><ColumnRuleStrokeColor type="object">Color/Near</ColumnRuleStrokeColor></Properties></TextFramePreference></ObjectStyle>
</RootObjectStyleGroup>
<Story Self="s"/><Spread Self="spread"><Page Self="page"/>
<TextFrame Self="local" ParentStory="s" AppliedObjectStyle="ObjectStyle/Base"><TextFramePreference><Properties><ColumnRuleStrokeColor type="object">Color/Near</ColumnRuleStrokeColor></Properties></TextFramePreference></TextFrame>
<TextFrame Self="inherited" ParentStory="s" AppliedObjectStyle="ObjectStyle/Child"/>
<TextFrame Self="attribute" ParentStory="s" AppliedObjectStyle="ObjectStyle/Child"><TextFramePreference ColumnRuleStrokeColor="Color/Far"><Properties><ColumnRuleStrokeColor type="object">Color/Near</ColumnRuleStrokeColor></Properties></TextFramePreference></TextFrame>
<TextFrame Self="invalid" ParentStory="s" AppliedObjectStyle="ObjectStyle/Child"><TextFramePreference><Properties><ColumnRuleStrokeColor type="object">missing</ColumnRuleStrokeColor></Properties></TextFramePreference></TextFrame>
</Spread></Document>"#;
    let d = import_idml_with(&zip_files(&[("designmap.xml", document)]), &|_| None).unwrap();
    let colors: Vec<_> = d.spreads[0].items.iter().map(|it| it.text_frame().unwrap().options.column_rule_color.as_str()).collect();
    assert_eq!(colors, vec!["Near", "Near", "Far", designcraft_color::swatch::NONE]);
    assert_eq!(d.styles.object_style("Child").unwrap().text_frame.as_ref().unwrap().column_rule_color, "Near");
}

/// A two-page spread with ruler guides on its pages (and one on the parent), in `origin` ruler
/// coordinates.
fn guides_package(origin: &str) -> Vec<u8> {
    let prefs = PREFS.replace("</idPkg:Preferences>", &format!(r#"<ViewPreference RulerOrigin="{origin}"/></idPkg:Preferences>"#));
    let spread = SPREAD.replace(
        r#"<Page Self="p2" AppliedMaster="m1" GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 0 -350"/>"#,
        r#"<Page Self="p2" AppliedMaster="m1" GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 0 -350">
      <Guide Self="g1" Orientation="Vertical" Location="40" FitToPage="true" ItemLayer="L1" Locked="true"/>
      <Guide Self="g2" Orientation="Horizontal" Location="100" FitToPage="false"/>
    </Page>"#,
    );
    let master = MASTER.replace(
        r#"<Page Self="mp1" GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 -500 -350"/>"#,
        r#"<Page Self="mp1" GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 -500 -350"><Guide Self="mg" Orientation="Vertical" Location="84"/></Page>"#,
    );
    zip_files(&[
        ("designmap.xml", DESIGNMAP),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Styles.xml", STYLES),
        ("Resources/Preferences.xml", &prefs),
        ("MasterSpreads/MasterSpread_m1.xml", &master),
        ("Spreads/Spread_sp1.xml", &spread),
        ("Stories/Story_s1.xml", STORY),
    ])
}

/// Ruler guides come in from IDML (they were dropped): on their page, measured from the ruler
/// origin, with Fit to Page, Locked and their layer; parent pages' too. They go out again.
#[test]
fn ruler_guides_import_and_round_trip() {
    use designcraft_doc::Orientation::{Horizontal, Vertical};
    let guides = |d: &Document| {
        let p = &d.spreads[0].pages[1];
        p.guides.iter().map(|g| (g.orientation, g.position, g.spread, g.locked, g.layer.is_some())).collect::<Vec<_>>()
    };
    // The second page starts at x 500 in our spread coordinates.
    let d = import_idml(&guides_package("PageOrigin")).unwrap();
    assert_eq!(guides(&d), [(Vertical, 540.0, false, true, true), (Horizontal, 100.0, true, false, false)]);
    let d = import_idml(&guides_package("SpineOrigin")).unwrap();
    assert_eq!(guides(&d)[0].1, 540.0, "the spine is the second page's left edge");
    let d = import_idml(&guides_package("SpreadOrigin")).unwrap();
    assert_eq!(guides(&d)[0].1, 40.0);
    assert_eq!(d.parents[0].pages[0].guides.len(), 1, "a parent page's guide");
    assert_eq!(d.parents[0].pages[0].guides[0].position, 84.0);
    // Round trip: written under each page from the document's ruler origin, read back where
    // they were.
    for origin in ["PageOrigin", "SpineOrigin", "SpreadOrigin"] {
        let d = import_idml(&guides_package(origin)).unwrap();
        let back = import_idml(&export_idml(&d)).unwrap();
        assert_eq!(guides(&back), guides(&d), "{origin}");
        assert_eq!(back.parents[0].pages[0].guides.len(), 1);
        assert_eq!(back.parents[0].pages[0].guides[0].position, d.parents[0].pages[0].guides[0].position, "{origin}");
    }
}

/// A package entry nothing in `designmap.xml` refers to is never decompressed: a damaged one
/// (its checksum no longer matches) does not stop the import.
#[test]
fn unreferenced_entries_are_not_read() {
    use zip::write::SimpleFileOptions;
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    w.start_file("mimetype", stored).unwrap();
    w.write_all(MIMETYPE.as_bytes()).unwrap();
    for (n, c) in [
        ("designmap.xml", DESIGNMAP),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Styles.xml", STYLES),
        ("Resources/Preferences.xml", PREFS),
        ("MasterSpreads/MasterSpread_m1.xml", MASTER),
        ("Spreads/Spread_sp1.xml", SPREAD),
        ("Stories/Story_s1.xml", STORY),
    ] {
        w.start_file(n, SimpleFileOptions::default()).unwrap();
        w.write_all(c.as_bytes()).unwrap();
    }
    w.start_file("junk/irrelevant.bin", stored).unwrap();
    w.write_all(b"<<payload-of-an-unused-entry>>").unwrap();
    let mut bytes = w.finish().unwrap().into_inner();
    let at = bytes.windows(9).position(|w| w == b"<<payload").unwrap();
    bytes[at] = b'X';
    let d = import_idml(&bytes).unwrap();
    assert_eq!(d.spreads.len(), 1);
}

/// A part that expands past the 512 MiB limit while claiming to hold one byte is refused once the
/// limit is reached, with an error that says so, instead of being read whole into memory.
#[test]
fn oversized_part_is_refused() {
    use zip::write::SimpleFileOptions;
    let designmap = r#"<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" Self="d">
          <idPkg:Story src="Stories/Story_big.xml"/>
        </Document>"#;
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    w.start_file("mimetype", SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)).unwrap();
    w.write_all(MIMETYPE.as_bytes()).unwrap();
    w.start_file("designmap.xml", SimpleFileOptions::default()).unwrap();
    w.write_all(designmap.as_bytes()).unwrap();
    w.start_file("Stories/Story_big.xml", SimpleFileOptions::default()).unwrap();
    let chunk = vec![b' '; 1 << 20];
    for _ in 0..512 {
        w.write_all(&chunk).unwrap();
    }
    w.write_all(b" ").unwrap();
    let mut bytes = w.finish().unwrap().into_inner();
    assert!(bytes.len() < 8 << 20, "{}", bytes.len());
    // Claim one byte in the local header and the central directory record (no ZIP64 fields or
    // data descriptors here), so only the bytes actually decompressed can show the part is too big.
    let mut patched = 0;
    for i in 0..bytes.len().saturating_sub(46) {
        let (size_at, len_at, name_at) = match &bytes[i..i + 4] {
            b"PK\x03\x04" => (22, 26, 30),
            b"PK\x01\x02" => (24, 28, 46),
            _ => continue,
        };
        let len = u16::from_le_bytes([bytes[i + len_at], bytes[i + len_at + 1]]) as usize;
        if bytes.get(i + name_at..i + name_at + len) == Some(b"Stories/Story_big.xml".as_slice()) {
            bytes[i + size_at..i + size_at + 4].copy_from_slice(&1u32.to_le_bytes());
            patched += 1;
        }
    }
    assert_eq!(patched, 2);
    let Err(err) = import_idml(&bytes) else { panic!("an oversized part imported") };
    assert!(err.to_string().contains("limit"), "{err}");
}

/// A package with more entries than any document needs is refused before any is read.
#[test]
fn too_many_entries_are_refused() {
    use zip::write::SimpleFileOptions;
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    w.start_file("mimetype", stored).unwrap();
    w.write_all(MIMETYPE.as_bytes()).unwrap();
    for i in 0..100_000 {
        w.start_file(format!("junk/{i}"), stored).unwrap();
    }
    let Err(err) = import_idml(&w.finish().unwrap().into_inner()) else { panic!("a package with 100 001 entries imported") };
    assert!(err.to_string().contains("entries"), "{err}");
}

/// A package whose [No paragraph style] carries the space-below leading model
/// (`LeadingModelAkiBelow`, written on every IDML) and applies `composer` in the
/// `<Properties><Composer>` form, with a 32 pt paragraph above a 24 pt one.
fn aki_below_doc(composer: &str) -> Document {
    let styles = STYLES.replace(
        r#"Name="$ID/[No paragraph style]" PointSize="12">
      <Properties>"#,
        &format!(
            r#"Name="$ID/[No paragraph style]" PointSize="12" LeadingModel="LeadingModelAkiBelow">
      <Properties><Composer type="string">{composer}</Composer>"#
        ),
    );
    let story = r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Story Self="s1">
      <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/[No paragraph style]">
        <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" PointSize="32"><Content>Big</Content><Br/></CharacterStyleRange>
      </ParagraphStyleRange>
      <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/[No paragraph style]">
        <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" PointSize="24"><Content>Small</Content></CharacterStyleRange>
      </ParagraphStyleRange></Story></idPkg:Story>"#;
    let pkg = zip_files(&[
        ("designmap.xml", DESIGNMAP),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Styles.xml", &styles),
        ("Resources/Preferences.xml", PREFS),
        ("MasterSpreads/MasterSpread_m1.xml", MASTER),
        ("Spreads/Spread_sp1.xml", SPREAD),
        ("Stories/Story_s1.xml", story),
    ]);
    import_idml(&pkg).unwrap()
}

/// The distance from the first baseline of [`aki_below_doc`]'s story to the second.
fn second_line_gap(d: &Document) -> f64 {
    let sid = d.stories.values().find(|s| s.text.contains("Big")).unwrap().id;
    let cs = designcraft_compose::compose_story(d, sid, &Default::default());
    let baselines: Vec<f64> = cs.frames.iter().flat_map(|f| &f.lines).map(|l| l.baseline).collect();
    assert_eq!(baselines.len(), 2, "{baselines:?}");
    baselines[1] - baselines[0]
}

/// The leading model on [No paragraph style] of every IDML takes effect only under the Japanese
/// composers: with the others, a 24 pt paragraph after a 32 pt one sits a Roman 28.8 pt (its auto
/// leading) below it.
#[test]
fn the_default_cjk_leading_model_applies_only_with_a_japanese_composer() {
    for composer in ["$ID/HL Composer", "$ID/HL Single", "$ID/HL Composer Optyca"] {
        let d = aki_below_doc(composer);
        let root = d.styles.para(designcraft_doc::NO_PARA_STYLE).unwrap();
        assert_eq!(root.chars.leading_model, Some(designcraft_doc::cjk::LeadingModel::AkiBelow), "the import keeps the file's model");
        let gap = second_line_gap(&d);
        assert!((gap - 28.8).abs() < 0.01, "{composer}: {gap}");
    }
    for composer in ["$ID/HL Composer J", "$ID/HL Single J"] {
        let gap = second_line_gap(&aki_below_doc(composer));
        assert!(gap > 28.8 + 1.0, "{composer}: the upper line's leading sits below it, not the lower line's ({gap})");
    }
}

/// A Japanese-composer document keeps its composer and leading model through export and
/// reimport, and lays out the same.
#[test]
fn a_japanese_composer_and_its_leading_model_round_trip() {
    use designcraft_doc::Composer as C;
    let d = aki_below_doc("$ID/HL Composer J");
    let back = import_idml(&export_idml(&d)).unwrap();
    for doc in [&d, &back] {
        let root = doc.styles.para(designcraft_doc::NO_PARA_STYLE).unwrap();
        assert_eq!(root.para.composer, Some(C::Japanese));
        assert_eq!(root.chars.leading_model, Some(designcraft_doc::cjk::LeadingModel::AkiBelow));
    }
    assert!((second_line_gap(&d) - second_line_gap(&back)).abs() < 0.01);
}

/// A composite-font metric `Document::check` refuses (zero, NaN, infinity) must
/// not make the whole package fail to open: the entry falls back to a usable value instead.
#[test]
fn unusable_cjk_metrics_do_not_refuse_the_whole_file() {
    let designmap = r#"<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" Self="d">
          <CompositeFont Self="cf" Name="Rubies">
            <CompositeFontEntry Self="e1" Name="Dot" AppliedFont="Myriad Pro" RelativeSize="0"/>
            <CompositeFontEntry Self="e2" Name="Bar" AppliedFont="Myriad Pro" BaselineShift="NaN"/>
            <CompositeFontEntry Self="e3" Name="Tiny" AppliedFont="Myriad Pro" RelativeSize="5e-324" HorizontalScale="5e-324" VerticalScale="5e-324"/>
          </CompositeFont>
          <idPkg:Spread src="Spreads/Spread_s.xml"/>
        </Document>"#;
    let spread = r#"<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
          <Spread Self="s"><Page Self="p" GeometricBounds="0 0 100 100" ItemTransform="1 0 0 1 0 0"/></Spread>
        </idPkg:Spread>"#;
    let d = import_idml(&zip_files(&[("designmap.xml", designmap), ("Spreads/Spread_s.xml", spread)])).unwrap();
    let entries = &d.styles.composite_fonts[0].entries;
    assert_eq!(entries[0].relative_size, 1.0, "an unusable relative size falls back to full size");
    assert_eq!(entries[1].baseline_shift, 0.0, "an unusable shift falls back to none");
    // Positive in the file but zero once read as a fraction.
    assert_eq!((entries[2].relative_size, entries[2].horizontal_scale, entries[2].vertical_scale), (1.0, 1.0, 1.0));
}

mod liquid_page_rules {
    use super::*;
    use designcraft_doc::LiquidRule as R;
    use std::sync::Arc;

    // Literal XML expectations, independent of names.rs and serde spellings.
    const CASES: [(&str, R); 6] = [
        ("Scale", R::Scale),
        ("Off", R::Off),
        ("Recenter", R::ReCenter),
        ("GuideBased", R::GuideBased),
        ("ObjectBased", R::ObjectBased),
        ("UseMaster", R::ControlledByParent),
    ];

    fn fixture(rule: Option<&str>, parent_rule: Option<&str>) -> Vec<u8> {
        let attribute = |value: Option<&str>| value.map(|v| format!(r#" LayoutRule="{v}""#)).unwrap_or_default();
        let (rule, parent_rule) = (attribute(rule), attribute(parent_rule));
        let map = r#"<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" Self="d">
  <idPkg:Preferences src="Resources/Preferences.xml"/>
  <idPkg:MasterSpread src="MasterSpreads/A.xml"/>
  <idPkg:MasterSpread src="MasterSpreads/B.xml"/>
  <idPkg:Spread src="Spreads/S.xml"/>
</Document>"#;
        let parent = format!(
            r#"<idPkg:MasterSpread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
  <MasterSpread Self="ma" NamePrefix="A" BaseName="Base" PageCount="2">
    <Page Self="a1" GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 -500 -350"/>
    <Page Self="a2"{parent_rule} GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 0 -350"/>
  </MasterSpread>
</idPkg:MasterSpread>"#
        );
        // Based-on belongs to the whole derived master: both pages apply the same master.
        let derived = format!(
            r#"<idPkg:MasterSpread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
  <MasterSpread Self="mb" NamePrefix="B" BaseName="Derived" PageCount="2">
    <Page Self="b1"{rule} AppliedMaster="ma" GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 -500 -350"/>
    <Page Self="b2" AppliedMaster="ma" GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 0 -350"/>
  </MasterSpread>
</idPkg:MasterSpread>"#
        );
        let spread = format!(
            r#"<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
  <Spread Self="s" PageCount="2">
    <Page Self="p1" AppliedMaster="ma" GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 -500 -350">
      <MarginPreference ColumnCount="3" ColumnGutter="12" Top="10" Bottom="20" Left="30" Right="40"/>
    </Page>
    <Page Self="p2"{rule} AppliedMaster="ma" GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 0 -350"/>
  </Spread>
</idPkg:Spread>"#
        );
        zip_files(&[
            ("designmap.xml", map),
            ("Resources/Preferences.xml", PREFS),
            ("MasterSpreads/A.xml", &parent),
            ("MasterSpreads/B.xml", &derived),
            ("Spreads/S.xml", &spread),
        ])
    }

    #[test]
    fn import_literal_xml() {
        let baseline = import_idml(&fixture(None, None)).unwrap();
        assert_eq!((baseline.spreads.len(), baseline.parents.len()), (1, 2));
        assert!(baseline.spreads.iter().chain(&baseline.parents).all(|sp| sp.pages.len() == 2));
        assert!(baseline.spreads.iter().chain(&baseline.parents).flat_map(|sp| &sp.pages).all(|pg| pg.liquid == R::Off));
        let cases = CASES.into_iter().map(|(literal, rule)| (Some(literal), rule)).chain([
            (None, R::Off),
            (Some("ReCenter"), R::Off),
            (Some("controlledByParent"), R::Off),
            (Some("PreserveExisting"), R::Off),
        ]);
        for (literal, rule) in cases {
            let actual = import_idml(&fixture(literal, Some("Recenter"))).unwrap();
            assert_eq!(actual.spreads[0].pages[1].liquid, rule, "ordinary Page: {literal:?}");
            assert_eq!(actual.parents[1].pages[0].liquid, rule, "master Page: {literal:?}");
            // Same XML without rule attributes supplies the complete allowed-delta oracle.
            let mut expected = baseline.clone();
            Arc::make_mut(&mut expected.spreads[0]).pages[1].liquid = rule;
            Arc::make_mut(&mut expected.parents[1]).pages[0].liquid = rule;
            Arc::make_mut(&mut expected.parents[0]).pages[1].liquid = R::ReCenter;
            // Import assigns a fresh creation timestamp; all other document fields are compared.
            expected.created = actual.created;
            assert_eq!(actual, expected, "{literal:?}: only the three explicit rule fields may change apart from the import timestamp");
            assert_eq!(actual.spreads[0].pages[0].liquid, R::Off, "omitted does not infer inheritance");
            assert_eq!(actual.spreads[0].pages[0].parent, Some(actual.parents[0].id));
            assert_eq!(actual.parents[1].parent.as_ref().unwrap().based_on, Some(actual.parents[0].id));
            assert!(actual.parents.iter().flat_map(|sp| &sp.pages).all(|pg| pg.parent.is_none()));
            if rule == R::ControlledByParent {
                // The right document page resolves to A's right page, not B's controlled page.
                assert_eq!(actual.parent_page_for(1), Some((0, 1)));
                assert_eq!(actual.parents[0].pages[1].liquid, R::ReCenter);
            }
        }
    }

    #[test]
    fn export_and_round_trip() {
        use std::io::Read;

        let baseline = import_idml(&fixture(None, None)).unwrap();
        for (literal, rule) in CASES {
            // Set native enums directly; the literal importer under test does not build this input.
            let mut source = baseline.clone();
            let pages = &mut Arc::make_mut(&mut source.spreads[0]).pages;
            pages[0].parent = None;
            // Without a parent there are no margins to follow, so they aren't local either.
            pages[0].local_margins = false;
            pages[0].liquid = R::ControlledByParent;
            pages[1].liquid = rule;
            Arc::make_mut(&mut source.parents[0]).pages[1].liquid = R::ReCenter;
            Arc::make_mut(&mut source.parents[1]).pages[0].liquid = rule;
            source.check().unwrap();
            let bytes = export_idml(&source);
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
            for (directory, tag, spread, rules) in [
                ("Spreads", "Spread", &source.spreads[0], ["UseMaster", literal]),
                ("MasterSpreads", "MasterSpread", &source.parents[0], ["Off", "Recenter"]),
                ("MasterSpreads", "MasterSpread", &source.parents[1], [literal, "Off"]),
            ] {
                let path = format!("{directory}/{tag}_u{:x}.xml", spread.id.0);
                let mut part = Vec::new();
                archive.by_name(&path).unwrap().read_to_end(&mut part).unwrap();
                let root = xml::parse(&part).unwrap();
                let body = root.find(tag).unwrap();
                let pages: Vec<_> = body.find_all("Page").collect();
                assert_eq!(pages.len(), spread.pages.len(), "{path}");
                for (page, expected) in spread.pages.iter().zip(rules) {
                    let id = format!("u{:x}", page.id.0);
                    let matches: Vec<_> = pages.iter().filter(|p| p.get("Self") == Some(id.as_str())).collect();
                    assert_eq!(matches.len(), 1, "{path}: {id}");
                    assert_eq!(matches[0].get("LayoutRule"), Some(expected), "{path}: {id}");
                }
            }
            let actual = import_idml(&bytes).unwrap();
            assert_eq!((actual.spreads.len(), actual.parents.len()), (1, 2));
            assert_eq!(actual.spreads[0].pages[0].parent, None);
            assert_eq!(actual.spreads[0].pages[1].parent, Some(actual.parents[0].id));
            assert_eq!(actual.parent_page_for(1), Some((0, 1)));
            assert!(actual.parents.iter().flat_map(|sp| &sp.pages).all(|pg| pg.parent.is_none()));
            for (index, prefix, name, based_on) in [(0, "A", "Base", None), (1, "B", "Derived", Some(actual.parents[0].id))] {
                let info = actual.parents[index].parent.as_ref().unwrap();
                assert_eq!((&info.prefix[..], &info.name[..], info.based_on), (prefix, name, based_on));
            }
            for (got, want) in actual.spreads.iter().chain(&actual.parents).zip(source.spreads.iter().chain(&source.parents)) {
                assert_eq!(got.pages.len(), want.pages.len());
                for (got, want) in got.pages.iter().zip(&want.pages) {
                    // IDs are allocated afresh; parent associations were checked explicitly above.
                    let mut normalized = got.clone();
                    normalized.id = want.id;
                    normalized.parent = want.parent;
                    assert_eq!(&normalized, want, "{literal}: all other Page fields, including the stored rule");
                }
            }
        }
    }
}

/// A dragged column gutter (InDesign's Custom columns) survives an IDML import, and an even set —
/// which InDesign writes for every page, moved or not — does not pretend to be custom (#1096).
#[test]
fn custom_column_positions_import_and_round_trip() {
    // Page 1 of SPREAD is 500 wide with Left=20 Right=30, so its margin box is 450 wide and
    // starts at x=30. Two columns with gutter 20 split evenly as "0 215 235 450".
    let with = |cols: &str| {
        let spread = SPREAD.replace(
            r#"<MarginPreference ColumnCount="3" ColumnGutter="12" Top="10" Bottom="10" Left="20" Right="30"/>"#,
            &format!(
                r#"<MarginPreference ColumnCount="2" ColumnGutter="20" Top="10" Bottom="10" Left="20" Right="30" ColumnDirection="Horizontal" {cols}/>"#
            ),
        );
        import_idml_with(
            &zip_files(&[
                ("designmap.xml", DESIGNMAP),
                ("Resources/Graphic.xml", GRAPHIC),
                ("Resources/Styles.xml", STYLES),
                ("Resources/Preferences.xml", PREFS),
                ("MasterSpreads/MasterSpread_m1.xml", MASTER),
                ("Spreads/Spread_sp1.xml", &spread),
                ("Stories/Story_s1.xml", STORY),
            ]),
            &|_| None,
        )
        .unwrap()
    };
    let rects = |d: &designcraft_doc::Document| d.spreads[0].pages[0].column_rects().iter().map(|r| (r.x0, r.x1)).collect::<Vec<_>>();

    // Dragged: the gutter sits at 150..170 instead of 215..235, and the columns follow it.
    let d = with(r#"ColumnsPositions="0 150 170 450""#);
    assert_eq!(d.spreads[0].pages[0].columns.positions, Some(vec![0.0, 150.0, 170.0, 450.0]));
    assert_eq!(rects(&d), vec![(30.0, 180.0), (200.0, 480.0)]);

    // The even set is dropped, so a later change of count or gutter is not frozen out. The
    // geometry is the same either way, which is what makes keeping it pointless.
    let even = with(r#"ColumnsPositions="0 215 235 450""#);
    assert_eq!(even.spreads[0].pages[0].columns.positions, None);
    assert_eq!(rects(&even), vec![(30.0, 245.0), (265.0, 480.0)]);
    // Including when InDesign's own arithmetic leaves the last bits different.
    assert_eq!(with(r#"ColumnsPositions="0 215.00000000000003 234.99999999999997 450""#).spreads[0].pages[0].columns.positions, None);

    // Malformed sets are ignored rather than half-read: an odd count, three columns' worth for a
    // count of two, a descending pair, a NaN, a set that stops short of the page's margin box, and
    // a list far longer than any count allows. Each page keeps the even split.
    let huge = format!(r#"ColumnsPositions="{}""#, (0..10_000).map(|i| i.to_string()).collect::<Vec<_>>().join(" "));
    for bad in [
        r#"ColumnsPositions="0 150 170""#,
        r#"ColumnsPositions="0 100 120 200 220 450""#,
        r#"ColumnsPositions="0 150 90 450""#,
        r#"ColumnsPositions="0 x 170 450""#,
        r#"ColumnsPositions="0 NaN 170 450""#,
        r#"ColumnsPositions="0 150 170 400""#,
        r#"ColumnsPositions="""#,
        huge.as_str(),
    ] {
        let d = with(bad);
        assert_eq!(d.spreads[0].pages[0].columns.positions, None, "{}", bad.get(..bad.len().min(60)).unwrap_or(""));
        assert_eq!(rects(&d), rects(&even));
    }

    // And they come back from an export, so the round trip keeps the dragged gutter.
    let back = import_idml(&export_idml(&d)).unwrap();
    assert_eq!(back.spreads[0].pages[0].columns.positions, Some(vec![0.0, 150.0, 170.0, 450.0]));
    assert_eq!(rects(&back), rects(&d));
    // A page nobody dragged still exports the even split, not a frozen copy of it.
    assert_eq!(import_idml(&export_idml(&even)).unwrap().spreads[0].pages[0].columns.positions, None);
}

/// #1096: a ruler guide inside the gutter of a two-column page, written the way InDesign writes
/// it (a `<Page>` child after its `MarginPreference`, with `Properties`/`GuideColor`, `PageIndex`
/// and `GuideZone`), in a document without facing pages. A spread guide sits beside it.
#[test]
fn ruler_guide_in_a_column_gutter_imports_without_facing_pages() {
    use designcraft_doc::Orientation::{Horizontal, Vertical};
    let package = |view: &str| {
        let prefs =
            PREFS.replace(r#"FacingPages="true""#, r#"FacingPages="false""#).replace("</idPkg:Preferences>", &format!("{view}</idPkg:Preferences>"));
        let spread = SPREAD.replace(
            r#"<Page Self="p1" AppliedMaster="m1" GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 -500 -350">
      <MarginPreference ColumnCount="3" ColumnGutter="12" Top="10" Bottom="10" Left="20" Right="30"/>
    </Page>"#,
            r#"<Page Self="p1" AppliedMaster="m1" GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 -500 -350" Name="1" OverrideList="">
      <Properties><PageColor type="enumeration">UseMasterColor</PageColor></Properties>
      <MarginPreference ColumnCount="2" ColumnGutter="20" Top="10" Bottom="10" Left="20" Right="30" ColumnDirection="Horizontal" ColumnsPositions="0 150 170 450"/>
      <GridDataInformation FontStyle="Regular" PointSize="12"/>
      <Guide Self="g1" OverriddenPageItemProps="" Orientation="Vertical" Location="180" FitToPage="true" ViewThreshold="0" Locked="false" ItemLayer="L1" PageIndex="0" GuideType="Ruler" GuideZone="0">
        <Properties><GuideColor type="enumeration">Cyan</GuideColor></Properties>
      </Guide>
      <Guide Self="g2" OverriddenPageItemProps="" Orientation="Horizontal" Location="300" FitToPage="false" ViewThreshold="5" Locked="false" ItemLayer="L1" PageIndex="0" GuideType="Ruler" GuideZone="1">
        <Properties><GuideColor type="enumeration">Cyan</GuideColor></Properties>
      </Guide>
    </Page>"#,
        );
        assert_ne!(spread, SPREAD, "the fixture's first page was replaced");
        import_idml(&zip_files(&[
            ("designmap.xml", DESIGNMAP),
            ("Resources/Graphic.xml", GRAPHIC),
            ("Resources/Styles.xml", STYLES),
            ("Resources/Preferences.xml", &prefs),
            ("MasterSpreads/MasterSpread_m1.xml", MASTER),
            ("Spreads/Spread_sp1.xml", &spread),
            ("Stories/Story_s1.xml", STORY),
        ]))
        .unwrap()
    };
    let guides =
        |d: &Document| d.spreads[0].pages[0].guides.iter().map(|g| (g.orientation, g.position, g.spread, g.layer.is_some())).collect::<Vec<_>>();
    // The first page starts at x 0 in our spread coordinates, so the spread's and the page's ruler
    // origins agree; no ViewPreference means InDesign's default, the spread.
    for view in ["", r#"<ViewPreference RulerOrigin="SpreadOrigin"/>"#, r#"<ViewPreference RulerOrigin="PageOrigin"/>"#] {
        let first = package(view);
        assert!(!first.settings.facing_pages);
        assert_eq!(guides(&first), [(Vertical, 180.0, false, true), (Horizontal, 300.0, true, true)], "{view:?}");
        // The guide sits between the page's left margin (20) and right margin (470).
        let m = first.spreads[0].pages[0].margin_rect();
        assert!(m.x0 < 180.0 && 180.0 < m.x1);
        let back = import_idml(&export_idml(&first)).unwrap();
        assert_eq!(guides(&back), guides(&first), "round trip, {view:?}");
    }
}

/// Cell Options › Text › First Baseline, set by a cell style that a table style applies to its body
/// region (cells with `[None]` take it from there) and overridden on one cell.
const FIRST_BASELINE_STYLES: &str = r#"<idPkg:Styles xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
<RootCellStyleGroup>
  <CellStyle Self="CellStyle/$ID/[None]" Name="$ID/[None]"/>
  <CellStyle Self="cs-cap" Name="Cap" FirstBaselineOffset="CapHeight" MinimumFirstBaselineOffset="2" VerticalJustification="CenterAlign"/>
  <CellStyle Self="cs-cap-child" Name="CapChild" FillTint="50"><Properties><BasedOn type="object">cs-cap</BasedOn></Properties></CellStyle>
</RootCellStyleGroup>
<RootParagraphStyleGroup/>
<RootTableStyleGroup>
  <TableStyle Self="ts" Name="Tbl" BodyRegionCellStyle="cs-cap-child"/>
</RootTableStyleGroup></idPkg:Styles>"#;

const FIRST_BASELINE_STORY: &str = r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
<Story Self="s1"><ParagraphStyleRange><CharacterStyleRange><Table AppliedTableStyle="ts" HeaderRowCount="0">
<Row Name="0" MinimumHeight="20"/>
<Column Name="0" SingleColumnWidth="60"/><Column Name="1" SingleColumnWidth="60"/>
<Cell Name="0:0" AppliedCellStyle="CellStyle/$ID/[None]"><ParagraphStyleRange><CharacterStyleRange><Content>Region</Content></CharacterStyleRange></ParagraphStyleRange></Cell>
<Cell Name="1:0" AppliedCellStyle="CellStyle/$ID/[None]" FirstBaselineOffset="LeadingOffset"><ParagraphStyleRange><CharacterStyleRange><Content>Local</Content></CharacterStyleRange></ParagraphStyleRange></Cell>
</Table></CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>"#;

#[test]
fn cell_first_baseline_comes_from_cell_styles_table_regions_and_cells_and_round_trips() {
    use designcraft_doc::FirstBaseline;
    let d = import_idml(&fixture_with_table_styles(FIRST_BASELINE_STYLES, FIRST_BASELINE_STORY)).unwrap();
    d.check().unwrap();
    let style = d.styles.cell.iter().find(|c| c.name == "Cap").unwrap();
    assert_eq!((style.first_baseline, style.first_baseline_min), (Some(FirstBaseline::CapHeight), Some(2.0)));
    let table = d.stories.values().flat_map(|s| s.tables.values()).next().unwrap();
    let region = table.cell(0, 0).unwrap();
    assert_eq!((region.first_baseline, region.first_baseline_min), (FirstBaseline::CapHeight, 2.0), "inherited through BasedOn and the body region");
    assert_eq!(table.cell(0, 1).unwrap().first_baseline, FirstBaseline::Leading, "a cell's own attribute wins");
    let back = import_idml(&export_idml(&d)).unwrap();
    let t2 = back.stories.values().flat_map(|s| s.tables.values()).next().unwrap();
    assert_eq!(t2.cell(0, 0).unwrap().first_baseline, FirstBaseline::CapHeight);
    assert_eq!(t2.cell(0, 0).unwrap().first_baseline_min, 2.0);
    assert_eq!(t2.cell(0, 1).unwrap().first_baseline, FirstBaseline::Leading);
    let s2 = back.styles.cell.iter().find(|c| c.name == "Cap").unwrap();
    assert_eq!(s2.first_baseline, Some(FirstBaseline::CapHeight));
}

/// A graphic placed in a table cell is written into the cell, not dropped.
#[test]
fn exports_graphic_in_table_cell() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(40.0, 40.0, 400.0, 400.0), lid, "Intro", ParaFormat::default()).unwrap();
    let asset = designcraft_doc::AssetId(d.alloc());
    let id = d.add_asset(designcraft_doc::Asset {
        id: asset,
        name: "tick.png".into(),
        mime: "image/png".into(),
        data: std::sync::Arc::new(vec![1, 2, 3]),
        ..Default::default()
    });
    let mut t = designcraft_doc::Table::new(d.alloc(), 2, 2, 0, 0, 200.0);
    t.cell_mut(1, 0).unwrap().graphic = Some(designcraft_doc::Graphic {
        layer_options: None,
        asset: id,
        size: (24.0, 24.0),
        xf: designcraft_geom::Affine::IDENTITY,
        clip: None,
        auto_fit: Default::default(),
        fit_align: 4,
        crop: [0.0; 4],
        wrap: Default::default(),
        fill: Default::default(),
    });
    d.story_mut(sid).unwrap().insert_table(5, t);
    let story = zip_text(&export_idml(&d), "Stories/");
    let cell = story.split("<Cell ").nth(3).unwrap_or_default();
    let cell = cell.split("</Cell>").next().unwrap_or_default();
    assert!(cell.contains("<Image "), "{story}");
    assert!(cell.contains("ClipContentToCell=\"true\""), "{story}");
}

/// Non-finite numbers in colour values, page bounds and item transforms (NaN, infinities and
/// values like 1e300 that overflow `f32`) never reach the model: JSON would write them as null.
#[test]
fn non_finite_number_lists_never_reach_the_model() {
    let graphic = GRAPHIC.replace(r#"ColorValue="255 0 0""#, r#"ColorValue="NaN 1e300 inf""#);
    let spread = SPREAD
        .replace(r#"Self="p2" AppliedMaster="m1" GeometricBounds="0 0 700 500""#, r#"Self="p2" AppliedMaster="m1" GeometricBounds="0 0 NaN 500""#)
        .replace(r#"ItemTransform="1 0 0 1 50 -300""#, r#"ItemTransform="1 0 0 1 inf -300""#);
    let bytes = zip_files(&[
        ("designmap.xml", DESIGNMAP),
        ("Resources/Graphic.xml", &graphic),
        ("Resources/Styles.xml", STYLES),
        ("Resources/Preferences.xml", PREFS),
        ("MasterSpreads/MasterSpread_m1.xml", MASTER),
        ("Spreads/Spread_sp1.xml", &spread),
        ("Stories/Story_s1.xml", STORY),
    ]);
    let doc = import_idml(&bytes).unwrap();
    for s in &doc.swatches {
        if let SwatchValue::Color { color, .. } | SwatchValue::Paper { color } = &s.value {
            let parts: Vec<f32> = match *color {
                Color::Rgb { r, g, b } => vec![r, g, b],
                Color::Cmyk { c, m, y, k } => vec![c, m, y, k],
                Color::Gray { k } => vec![k],
            };
            assert!(parts.iter().all(|v| v.is_finite()), "swatch {} is not finite: {color:?}", s.name);
        }
    }
    for sp in doc.spreads.iter().chain(&doc.parents) {
        for p in &sp.pages {
            assert!(p.width.is_finite() && p.height.is_finite() && p.x.is_finite(), "page {p:?}");
        }
    }
    assert_eq!(doc.non_finite_item(), None);
}

/// Cell Options › Text › Rotation: the angle survives the round trip, and the import says it is
/// not composed — narrow header columns of vertical labels come in horizontal and wrap mid-word,
/// and nothing said so (#1468).
#[test]
fn rotated_cell_text_round_trips_and_the_import_says_it_is_not_composed() {
    let mut d = Document::new(&NewDocument::default());
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 300.0, 300.0), d.default_layer(), "", ParaFormat::default()).unwrap();
    let mut t = designcraft_doc::Table::new(7, 1, 3, 0, 0, 300.0);
    t.cell_mut(0, 0).unwrap().rotation = 90.0;
    t.cell_mut(0, 1).unwrap().rotation = 270.0;
    d.story_mut(sid).unwrap().insert_table(0, t);
    let bytes = export_idml(&d);
    let story = zip_text(&bytes, "Stories/");
    assert!(story.contains(r#"RotationAngle="90""#) && story.contains(r#"RotationAngle="270""#), "{story}");

    let back = import_idml_report(&bytes, &|_| None).unwrap();
    let t = back.document.stories.values().find(|st| !st.tables.is_empty()).unwrap().tables.values().next().unwrap().clone();
    assert_eq!((t.cell(0, 0).unwrap().rotation, t.cell(0, 1).unwrap().rotation, t.cell(0, 2).unwrap().rotation), (90.0, 270.0, 0.0));
    let about: Vec<&String> = back.warnings.iter().filter(|w| w.contains("rotated text")).collect();
    assert_eq!(about.len(), 1, "one line for the file, not one per cell: {:?}", back.warnings);
    assert!(about[0].starts_with("2 table cell(s)"), "{}", about[0]);
    assert!(about[0].contains("composed horizontally"), "{}", about[0]);

    // A file with no rotated cell says nothing.
    let mut plain = Document::new(&NewDocument::default());
    let (_, sid) =
        plain.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 300.0, 300.0), plain.default_layer(), "", ParaFormat::default()).unwrap();
    plain.story_mut(sid).unwrap().insert_table(0, designcraft_doc::Table::new(8, 1, 2, 0, 0, 200.0));
    let quiet = import_idml_report(&export_idml(&plain), &|_| None).unwrap();
    assert!(quiet.warnings.iter().all(|w| !w.contains("rotated text")), "{:?}", quiet.warnings);
}

#[test]
fn style_names_with_a_literal_colon_survive_a_round_trip() {
    use designcraft_doc::styles::{CellStyle, CharacterStyle, ObjectStyle, ParagraphStyle, TableStyle};
    let names = ["Title: Big", "Group/Child: X", "Back\\slash: \\:"];
    let mut d = small_doc();
    for n in names {
        let st = d.styles_mut();
        st.paragraph.push(ParagraphStyle {
            name: n.into(),
            based_on: None,
            next_style: None,
            para: Default::default(),
            chars: Default::default(),
            shortcut: String::new(),
        });
        st.character.push(CharacterStyle { name: n.into(), based_on: None, chars: Default::default(), shortcut: String::new() });
        st.cell.push(CellStyle { name: n.into(), ..Default::default() });
        st.table.push(TableStyle { name: n.into(), ..Default::default() });
        st.object.push(ObjectStyle { name: n.into(), ..Default::default() });
    }
    let back = import_idml(&export_idml(&d)).unwrap();
    for n in names {
        assert!(
            back.styles.paragraph.iter().any(|s| s.name == n),
            "paragraph {n:?}: {:?}",
            back.styles.paragraph.iter().map(|s| &s.name).collect::<Vec<_>>()
        );
        assert!(back.styles.character.iter().any(|s| s.name == n), "character {n:?}");
        assert!(back.styles.cell.iter().any(|s| s.name == n), "cell {n:?}");
        assert!(back.styles.table.iter().any(|s| s.name == n), "table {n:?}");
        assert!(back.styles.object.iter().any(|s| s.name == n), "object {n:?}");
    }
    // A group is still written as an IDML `:` group path.
    assert_eq!(names::style_name_out(names::PARA_BUILTINS, "Group/Child"), "Group:Child");
    for n in names {
        assert_eq!(names::style_name_in(names::PARA_BUILTINS, &names::style_name_out(names::PARA_BUILTINS, n)), n);
    }
}

/// Split Document (EPUB only) comes from the style, or from its EPUB export tag map, and is
/// written back on the style.
#[test]
fn split_document_round_trips() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<idPkg:Styles xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <RootParagraphStyleGroup Self="rp">
    <ParagraphStyle Self="ParagraphStyle/$ID/[No paragraph style]" Name="$ID/[No paragraph style]" PointSize="12" SplitDocument="false"/>
    <ParagraphStyle Self="ParagraphStyle/Part" Name="Part" SplitDocument="true"/>
    <ParagraphStyle Self="ParagraphStyle/Chapter" Name="Chapter" SplitDocument="false">
      <StyleExportTagMap Self="uaStyleExportTagMapnEPUB" ExportType="EPUB" ExportTag="h1" ExportClass="" ExportAttributes="" SplitDocument="true" EmitCss="true"/>
    </ParagraphStyle>
    <ParagraphStyle Self="ParagraphStyle/Body" Name="Body" SplitDocument="false"/>
  </RootParagraphStyleGroup>
</idPkg:Styles>"#;
    let d = import_idml_with(&fixture_with_styles(xml), &|_| None).unwrap();
    let split = |d: &designcraft_doc::Document, name: &str| d.styles.export_tag(name, false).is_some_and(|t| t.split);
    assert!(split(&d, "Part") && split(&d, "Chapter") && !split(&d, "Body"));
    let back = import_idml(&export_idml(&d)).unwrap();
    assert!(split(&back, "Part") && split(&back, "Chapter") && !split(&back, "Body"));
}

/// EPUB Export Options › Split Document: off, one style (built in or not) or the styles' export
/// tags, read from the file's `EPubExportPreference` and written back.
#[test]
fn epub_split_document_round_trips() {
    use designcraft_doc::EpubSplit;
    let styles =
        STYLES.replace("</RootParagraphStyleGroup>", r#"<ParagraphStyle Self="ParagraphStyle/Chapter" Name="Chapter"/></RootParagraphStyleGroup>"#);
    assert!(styles.contains("ParagraphStyle/Chapter"), "the fixture has a paragraph style group");
    for (attrs, want) in [
        (r#"BreakDocument="false" ParagraphStyleName="ParagraphStyle/Chapter""#, EpubSplit::Off),
        (r#"BreakDocument="true" ParagraphStyleName="ParagraphStyle/Chapter""#, EpubSplit::Style("Chapter".into())),
        (r#"BreakDocument="true" ParagraphStyleName="ParagraphStyle/Text%3aBody""#, EpubSplit::Style("Text/Body".into())),
        (r#"BreakDocument="true" ParagraphStyleName="$ID/NormalParagraphStyle""#, EpubSplit::Style(designcraft_doc::BASIC_PARAGRAPH.into())),
        (r#"BreakDocument="true" ParagraphStyleName="""#, EpubSplit::ExportTags),
        (r#"BreakDocument="true" ParagraphStyleName="ParagraphStyle/Missing""#, EpubSplit::Off),
    ] {
        let prefs = PREFS.replace("</idPkg:Preferences>", &format!("<EPubExportPreference {attrs}/></idPkg:Preferences>"));
        let file = zip_files(&[
            ("designmap.xml", DESIGNMAP),
            ("Resources/Graphic.xml", GRAPHIC),
            ("Resources/Styles.xml", &styles),
            ("Resources/Preferences.xml", &prefs),
            ("MasterSpreads/MasterSpread_m1.xml", MASTER),
            ("Spreads/Spread_sp1.xml", SPREAD),
            ("Stories/Story_s1.xml", STORY),
        ]);
        let d = import_idml_with(&file, &|_| None).unwrap();
        assert_eq!(d.settings.epub_split, want, "{attrs}");
        assert_eq!(import_idml(&export_idml(&d)).unwrap().settings.epub_split, want, "{attrs}");
    }
}
