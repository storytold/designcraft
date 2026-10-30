//! The XML structure: InDesign's `XMLElement`s (the root in the backing story, story, table and
//! cell elements, inline elements with attributes) are kept, and U+FEFF stays in the text.

use designcraft_doc::{XmlContent, XmlElement};

use super::*;

const BACKING: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<idPkg:BackingStory xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <XmlStory Self="bs">
    <ParagraphStyleRange><CharacterStyleRange><Content>&#xFEFF;</Content>
      <XMLElement Self="di2" MarkupTag="XMLTag/Root">
        <XMLElement Self="di2i3" MarkupTag="XMLTag/Story" XMLContent="s1" />
      </XMLElement>
    </CharacterStyleRange></ParagraphStyleRange>
  </XmlStory>
</idPkg:BackingStory>"#;

const TAGS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<idPkg:Tags xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <XMLTag Self="XMLTag/Root" Name="Root"><Properties><TagColor type="enumeration">LightBlue</TagColor></Properties></XMLTag>
  <XMLTag Self="XMLTag/Translation" Name="Translation"><Properties><TagColor type="enumeration">Red</TagColor></Properties></XMLTag>
</idPkg:Tags>"#;

const TAGGED_STORY: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <Story Self="s1">
    <XMLElement Self="di2i3" MarkupTag="XMLTag/Story" XMLContent="s1">
      <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/Text%3aBody">
        <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>Intro </Content></CharacterStyleRange>
        <XMLElement Self="di2i3i4" MarkupTag="XMLTag/Translation">
          <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" FontStyle="Bold"><Content>Neu</Content></CharacterStyleRange>
          <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content> heute</Content></CharacterStyleRange>
          <XMLAttribute Self="di2i3i4a1" Name="Id" Value="1000" />
          <XMLAttribute Self="di2i3i4a2" Name="SourceText" Value="Neu heute" />
        </XMLElement>
        <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content> &#xFEFF;54&#xFEFF;</Content><Br /></CharacterStyleRange>
        <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]">
          <XMLElement Self="di2i3i5" MarkupTag="XMLTag/Table" XMLContent="tb1">
            <Table Self="tb1" HeaderRowCount="0" FooterRowCount="0" BodyRowCount="1" ColumnCount="1">
              <Row Self="tb1Row0" Name="0" SingleRowHeight="20" />
              <Column Self="tb1Column0" Name="0" SingleColumnWidth="100" />
              <Cell Self="tb1i0" Name="0:0" RowSpan="1" ColumnSpan="1">
                <XMLElement Self="di2i3i5i6" MarkupTag="XMLTag/Cell" XMLContent="tb1i0">
                  <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/Text%3aBody">
                    <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]">
                      <XMLElement Self="di2i3i5i6i7" MarkupTag="XMLTag/Translation">
                        <Content>Motiv 498</Content>
                        <XMLAttribute Self="di2i3i5i6i7a1" Name="Id" Value="1001" />
                      </XMLElement>
                    </CharacterStyleRange>
                  </ParagraphStyleRange>
                </XMLElement>
              </Cell>
            </Table>
          </XMLElement>
        </CharacterStyleRange>
      </ParagraphStyleRange>
    </XMLElement>
  </Story>
</idPkg:Story>"#;

fn tagged() -> Document {
    let designmap = DESIGNMAP.replace(
        "  <idPkg:Story src=\"Stories/Story_s1.xml\"/>",
        "  <idPkg:Tags src=\"XML/Tags.xml\"/>\n  <idPkg:BackingStory src=\"XML/BackingStory.xml\"/>\n  <idPkg:Story src=\"Stories/Story_s1.xml\"/>",
    );
    let bytes = zip_files(&[
        ("designmap.xml", &designmap),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Styles.xml", STYLES),
        ("Resources/Preferences.xml", PREFS),
        ("MasterSpreads/MasterSpread_m1.xml", MASTER),
        ("Spreads/Spread_sp1.xml", SPREAD),
        ("Stories/Story_s1.xml", TAGGED_STORY),
        ("XML/BackingStory.xml", BACKING),
        ("XML/Tags.xml", TAGS),
    ]);
    import_idml(&bytes).unwrap()
}

fn by_tag<'d>(d: &'d Document, tag: &str) -> Vec<(u64, &'d XmlElement)> {
    d.xml.elements.iter().filter(|(_, e)| e.tag == tag).map(|(id, e)| (*id, e)).collect()
}

#[test]
fn xml_elements_are_read_with_their_content_and_attributes() {
    let d = tagged();
    assert_eq!(d.xml.root, "Root");
    assert!(d.xml.tags.iter().any(|t| t.name == "Translation"));
    let story = d.stories.values().find(|s| s.text.contains("Intro")).unwrap();
    // The root's one child is the story's element, holding that story.
    let [(story_el, se)] = by_tag(&d, "Story")[..] else { panic!("one Story element") };
    assert_eq!(d.xml.root_children, vec![story_el]);
    assert_eq!((se.content, se.parent), (XmlContent::Story { story: story.id }, 0));
    // The inline element keeps its attributes in order and tags its text, across two ranges.
    let translations = by_tag(&d, "Translation");
    let (neu, ne) = translations.iter().find(|(_, e)| e.attr("Id") == Some("1000")).copied().unwrap();
    assert_eq!(ne.attrs, vec![("Id".to_string(), "1000".to_string()), ("SourceText".into(), "Neu heute".into())]);
    assert_eq!((ne.content, ne.parent), (XmlContent::Text, story_el));
    let tagged: String = story.runs().filter(|(_, f)| f.over.xml_element == Some(neu)).map(|(r, _)| &story.text[r]).collect();
    assert_eq!(tagged, "Neu heute");
    assert!(story.runs().any(|(r, f)| f.over.xml_element == Some(neu) && &story.text[r] == "Neu"), "formatting ranges stay apart");
    // U+FEFF stays in the text.
    assert!(story.text.contains("\u{FEFF}54\u{FEFF}"), "{:?}", story.text);
    // The table and its cell are elements by content; the cell's Translation is in its text.
    let [(table_el, te)] = by_tag(&d, "Table")[..] else { panic!("one Table element") };
    let (tid, table) = story.tables.iter().next().unwrap();
    assert!(matches!(te.content, XmlContent::Table { table, .. } if table == *tid));
    let [(_, ce)] = by_tag(&d, "Cell")[..] else { panic!("one Cell element") };
    assert_eq!((ce.content, ce.parent), (XmlContent::Cell { table: *tid, cell: 0 }, table_el));
    let (cell_tr, _) = translations.iter().find(|(_, e)| e.attr("Id") == Some("1001")).copied().unwrap();
    let cell = &table.cells[0].text;
    assert!(cell.runs().any(|(r, f)| f.over.xml_element == Some(cell_tr) && &cell.text[r] == "Motiv 498"));
}
