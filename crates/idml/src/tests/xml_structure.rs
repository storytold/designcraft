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
    tagged_with(BACKING, TAGGED_STORY)
}

/// The tagged fixture with another backing story or story.
fn tagged_with(backing: &str, story: &str) -> Document {
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
        ("Stories/Story_s1.xml", story),
        ("XML/BackingStory.xml", backing),
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

/// Each element as (tag, attributes, content kind, parent's tag, its text), sorted: what an IDML
/// round trip has to keep.
fn structure(d: &Document) -> Vec<(String, Vec<(String, String)>, &'static str, String, String)> {
    let text_of = |id: u64| -> String {
        let mut out = String::new();
        let mut stories: Vec<&designcraft_doc::Story> = d.stories.values().map(|s| &**s).collect();
        for s in d.stories.values() {
            for t in s.tables.values() {
                stories.extend(t.cells.iter().map(|c| &c.text));
            }
        }
        for s in stories {
            out.extend(s.runs().filter(|(_, f)| f.over.xml_element == Some(id)).map(|(r, _)| s.text[r].to_string()));
        }
        out
    };
    let mut v: Vec<_> = d
        .xml
        .elements
        .iter()
        .map(|(id, e)| {
            let kind = match e.content {
                XmlContent::None => "none",
                XmlContent::Story { .. } => "story",
                XmlContent::Table { .. } => "table",
                XmlContent::Cell { .. } => "cell",
                XmlContent::Text => "text",
            };
            let parent = d.xml.elements.get(&e.parent).map_or("(root)".to_string(), |p| p.tag.clone());
            (e.tag.clone(), e.attrs.clone(), kind, parent, text_of(*id))
        })
        .collect();
    v.sort();
    v
}

#[test]
fn idml_export_keeps_the_xml_structure() {
    let d = tagged();
    let bytes = export_idml(&d);
    let back = import_idml(&bytes).unwrap();
    assert_eq!(structure(&back), structure(&d));
    assert_eq!(back.xml.root, "Root");
    assert_eq!(back.xml.root_children.len(), 1);
    let story = back.stories.values().find(|s| s.text.contains("Intro")).unwrap();
    assert_eq!(story.text, d.stories.values().find(|s| s.text.contains("Intro")).unwrap().text, "the text stays as it was");
    // The tags keep their colours; the package names the XML parts as InDesign does.
    assert_eq!(
        back.xml.tags.iter().find(|t| t.name == "Translation").map(|t| t.color),
        d.xml.tags.iter().find(|t| t.name == "Translation").map(|t| t.color)
    );
    let designmap = zip_text(&bytes, "designmap.xml");
    assert!(
        designmap.contains(r#"<idPkg:Tags src="XML/Tags.xml"/>"#) && designmap.contains(r#"<idPkg:BackingStory src="XML/BackingStory.xml"/>"#),
        "{designmap}"
    );
}

#[test]
fn inline_elements_sit_where_indesign_puts_them() {
    let bytes = export_idml(&tagged());
    let story = zip_text(&bytes, "Stories/");
    let root = crate::xml::parse(story.split("<?xml").find(|p| p.contains("Intro")).map(|p| format!("<?xml{p}")).unwrap().as_bytes()).unwrap();
    // (the element's tag, the tag of the element it is written in)
    fn walk(e: &crate::xml::El, out: &mut Vec<(String, String)>) {
        for c in e.elements() {
            if c.name == "XMLElement" {
                out.push((c.get("MarkupTag").unwrap_or("").to_string(), e.name.clone()));
            }
            walk(c, out);
        }
    }
    let mut placed = Vec::new();
    walk(&root, &mut placed);
    let place = |tag: &str| placed.iter().filter(|(t, _)| t == tag).map(|(_, p)| p.as_str()).collect::<Vec<_>>();
    assert_eq!(place("XMLTag/Story"), ["Story"], "around the story's text");
    // "Neu heute" spans two character ranges: around them in the paragraph; "Motiv 498" fits one.
    assert_eq!(place("XMLTag/Translation"), ["ParagraphStyleRange", "CharacterStyleRange"], "{placed:?}");
    assert_eq!(place("XMLTag/Table"), ["CharacterStyleRange"], "around the table, in its anchor's range");
    assert_eq!(place("XMLTag/Cell"), ["Cell"]);
    // Attributes follow the content, as InDesign writes them; the paragraph break stays outside.
    let neu = story.find(r#"Name="Id" Value="1000""#).unwrap();
    assert!(story[..neu].rfind("heute").is_some_and(|h| story[h..neu].find("<Br").is_none()), "{story}");
    let backing = zip_text(&bytes, "XML/BackingStory.xml");
    assert!(backing.contains(r#"MarkupTag="XMLTag/Story" XMLContent="#) && backing.contains("\u{feff}"), "{backing}");
}

#[test]
fn a_document_without_xml_writes_no_xml_parts() {
    let d = import_idml(&fixture()).unwrap();
    assert!(d.xml.elements.is_empty());
    let bytes = export_idml(&d);
    assert!(zip_text(&bytes, "XML/").is_empty());
    assert!(!zip_text(&bytes, "designmap.xml").contains("BackingStory"));
}

/// A tag colour that isn't one of the named ones is written as a list and read back as one.
#[test]
fn a_custom_tag_colour_comes_back() {
    let mut d = tagged();
    if let Some(t) = d.xml.tags.iter_mut().find(|t| t.name == "Translation") {
        t.color = [10, 20, 30];
    }
    let back = import_idml(&export_idml(&d)).unwrap();
    assert_eq!(back.xml.tags.iter().find(|t| t.name == "Translation").map(|t| t.color), Some([10, 20, 30]));
}

/// A hyperlink over untagged text and an inline element: the element stays around its own text
/// inside the link source, and the untagged text stays out of it.
#[test]
fn a_hyperlink_over_text_of_different_elements_keeps_them_apart() {
    let mut d = tagged();
    let sid = d.stories.values().find(|s| s.text.contains("Intro")).unwrap().id;
    // "Intro " (untagged) and "Neu heute" (Translation 1000)
    d.hyperlinks.push(designcraft_doc::Hyperlink {
        id: 1,
        name: "Link".into(),
        source: designcraft_doc::HyperlinkSource::Text { story: sid, start: 0, end: 15 },
        dest: designcraft_doc::HyperlinkDest::Url("https://example.com".into()),
    });
    let bytes = export_idml(&d);
    assert!(zip_text(&bytes, "Stories/").contains("HyperlinkTextSource"), "the link is written");
    assert_eq!(structure(&import_idml(&bytes).unwrap()), structure(&d));
}

/// A story's element that the backing story lists inside a wrapper element is that element, under
/// the wrapper – not a second, empty one.
#[test]
fn a_story_element_inside_a_root_wrapper_stays_one_element() {
    let backing = BACKING.replace(
        r#"<XMLElement Self="di2i3" MarkupTag="XMLTag/Story" XMLContent="s1" />"#,
        r#"<XMLElement Self="di2w" MarkupTag="XMLTag/Wrap"><XMLElement Self="di2i3" MarkupTag="XMLTag/Story" XMLContent="s1" /></XMLElement>"#,
    );
    let d = tagged_with(&backing, TAGGED_STORY);
    let stories = by_tag(&d, "Story");
    let [(wrap, we)] = by_tag(&d, "Wrap")[..] else { panic!("one Wrap element") };
    assert_eq!(stories.len(), 1, "no second Story element");
    assert_eq!(d.xml.root_children, vec![wrap]);
    assert_eq!((stories[0].1.parent, &we.children), (wrap, &vec![stories[0].0]));
    assert!(matches!(stories[0].1.content, XmlContent::Story { .. }));
}

/// Elements inside one whose content isn't kept (a page item) stay as its children.
#[test]
fn elements_inside_content_that_is_not_kept_stay() {
    let story = TAGGED_STORY.replace(
        r#"<CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>Intro </Content></CharacterStyleRange>"#,
        r#"<XMLElement Self="di2i3i9" MarkupTag="XMLTag/Pic" XMLContent="u999"><XMLElement Self="di2i3i9i1" MarkupTag="XMLTag/Caption"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>Intro </Content></CharacterStyleRange></XMLElement></XMLElement>"#,
    );
    assert_ne!(story, TAGGED_STORY);
    let d = tagged_with(BACKING, &story);
    let [(_, pic)] = by_tag(&d, "Pic")[..] else { panic!("one Pic element") };
    let [(caption, _)] = by_tag(&d, "Caption")[..] else { panic!("one Caption element") };
    assert_eq!((pic.content, &pic.children), (XmlContent::None, &vec![caption]));
}
