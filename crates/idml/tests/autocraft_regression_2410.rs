//! A table cell's insets written as `TextTopInset` ... (current InDesign) are read, and a cell
//! style's too; they survive a round trip.

use std::io::Write;

use designcraft_doc::{Document, Table};
use designcraft_idml::{MIMETYPE, export_idml, import_idml};

const DESIGNMAP: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0" Self="d" StoryList="s1" Name="Fixture.indd">
  <idPkg:Styles src="Resources/Styles.xml"/>
  <idPkg:Spread src="Spreads/Spread_sp1.xml"/>
  <idPkg:Story src="Stories/Story_s1.xml"/>
</Document>"#;

const STYLES: &str = r#"<idPkg:Styles xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
<RootCellStyleGroup>
  <CellStyle Self="CellStyle/$ID/[None]" Name="$ID/[None]"/>
  <CellStyle Self="cs-pad" Name="Pad" TextTopInset="6" TextLeftInset="7" TextBottomInset="8" TextRightInset="9"/>
</RootCellStyleGroup>
</idPkg:Styles>"#;

const SPREAD: &str = r#"<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <Spread Self="sp1" PageCount="1" ItemTransform="1 0 0 1 0 0">
    <Page Self="p1" GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 -250 -350"/>
    <TextFrame Self="t1" ParentStory="s1" PreviousTextFrame="n" NextTextFrame="n" ItemTransform="1 0 0 1 -200 -300">
      <Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
        <PathPointType Anchor="0 0"/><PathPointType Anchor="0 200"/><PathPointType Anchor="300 200"/><PathPointType Anchor="300 0"/>
      </PathPointArray></GeometryPathType></PathGeometry></Properties>
    </TextFrame>
  </Spread>
</idPkg:Spread>"#;

/// Cells: 0 uses the current names, 1 carries both (the current ones win), 2 only the older
/// names, 3 takes its insets from a cell style written with the current names.
const STORY: &str = r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
<Story Self="s1"><ParagraphStyleRange><CharacterStyleRange><Table HeaderRowCount="0">
<Row Name="0" MinimumHeight="24"/>
<Column Name="0" SingleColumnWidth="60"/><Column Name="1" SingleColumnWidth="60"/><Column Name="2" SingleColumnWidth="60"/><Column Name="3" SingleColumnWidth="60"/>
<Cell Name="0:0" TextTopInset="11" TextLeftInset="12" TextBottomInset="13" TextRightInset="14"><ParagraphStyleRange><CharacterStyleRange><Content>A</Content></CharacterStyleRange></ParagraphStyleRange></Cell>
<Cell Name="1:0" TextTopInset="21" TopInset="1" LeftInset="2"><ParagraphStyleRange><CharacterStyleRange><Content>B</Content></CharacterStyleRange></ParagraphStyleRange></Cell>
<Cell Name="2:0" TopInset="31" LeftInset="32" BottomInset="33" RightInset="34"><ParagraphStyleRange><CharacterStyleRange><Content>C</Content></CharacterStyleRange></ParagraphStyleRange></Cell>
<Cell Name="3:0" AppliedCellStyle="cs-pad"><ParagraphStyleRange><CharacterStyleRange><Content>D</Content></CharacterStyleRange></ParagraphStyleRange></Cell>
</Table></CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>"#;

fn package() -> Vec<u8> {
    use zip::write::SimpleFileOptions;
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    w.start_file("mimetype", stored).unwrap();
    w.write_all(MIMETYPE.as_bytes()).unwrap();
    for (name, content) in
        [("designmap.xml", DESIGNMAP), ("Resources/Styles.xml", STYLES), ("Spreads/Spread_sp1.xml", SPREAD), ("Stories/Story_s1.xml", STORY)]
    {
        w.start_file(name, SimpleFileOptions::default()).unwrap();
        w.write_all(content.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn table(doc: &Document) -> &Table {
    doc.stories.values().flat_map(|s| s.tables.values()).next().unwrap()
}

fn insets(doc: &Document) -> Vec<[f64; 4]> {
    let t = table(doc);
    (0..4).map(|c| t.cell(0, c).unwrap().insets).collect()
}

#[test]
fn current_text_inset_names_are_read_and_round_trip() {
    let doc = import_idml(&package()).unwrap();
    let got = insets(&doc);
    assert_eq!(got[0], [11.0, 12.0, 13.0, 14.0], "a cell's current-named insets");
    assert_eq!(got[2], [31.0, 32.0, 33.0, 34.0], "a cell's older-named insets still work");
    assert_eq!(got[3], [6.0, 7.0, 8.0, 9.0], "a cell style's current-named insets");
    assert_eq!(got[1][0], 21.0, "the current name wins over the older one");
    assert_eq!(got[1][1], 2.0, "a side only the older name sets falls back to it");

    let back = import_idml(&export_idml(&doc)).unwrap();
    assert_eq!(insets(&back), got, "the insets survive a round trip");
}
