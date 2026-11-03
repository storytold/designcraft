//! ICML import and export: hand-written files in the shapes InDesign/InCopy and Pandoc write, and
//! stories exported and read back.

use std::sync::Arc;

use designcraft_color::{Color, Swatch};
use designcraft_doc::build::NewDocument;
use designcraft_doc::{
    AnchorPosition, AnchoredObject, Asset, AssetId, CharAttrs, CharacterStyle, Content, CrossRef, Document, Item, ItemId, ParaAttrs, ParaFormat,
    ParagraphStyle, Shape, SpreadRef, Story, Table,
};
use designcraft_geom::{Rect, shapes};

use super::*;
use crate::export_icml;

/// A 1 × 1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

/// An ICML file in the shape Pandoc writes: styles with a `$ID/` character style named
/// "Default", `<Br/>` between the paragraph ranges, a hyperlink source around text, and the
/// hyperlink after the story.
const PANDOC_STYLE: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<?aid style="50" type="snippet" readerVersion="6.0" featureSet="513" product="8.0(370)" ?>
<?aid SnippetType="InCopyInterchange"?>
<Document DOMVersion="8.0" Self="conv_doc">
  <RootCharacterStyleGroup Self="conv_character_styles">
    <CharacterStyle Self="$ID/NormalCharacterStyle" Name="Default" />
    <CharacterStyle Self="CharacterStyle/Italic" Name="Italic" FontStyle="Italic">
      <Properties><BasedOn type="object">$ID/NormalCharacterStyle</BasedOn></Properties>
    </CharacterStyle>
  </RootCharacterStyleGroup>
  <RootParagraphStyleGroup Self="conv_paragraph_styles">
    <ParagraphStyle Self="$ID/NormalParagraphStyle" Name="$ID/NormalParagraphStyle" SpaceBefore="6" SpaceAfter="6" />
    <ParagraphStyle Self="ParagraphStyle/Header1" Name="Header1" PointSize="36">
      <Properties><BasedOn type="object">$ID/NormalParagraphStyle</BasedOn></Properties>
    </ParagraphStyle>
    <ParagraphStyle Self="ParagraphStyle/Paragraph" Name="Paragraph" FirstLineIndent="12">
      <Properties><BasedOn type="object">$ID/NormalParagraphStyle</BasedOn></Properties>
    </ParagraphStyle>
  </RootParagraphStyleGroup>
  <Story Self="conv_story" TrackChanges="false" StoryTitle="" AppliedTOCStyle="n" AppliedNamedGrid="n">
    <StoryPreference OpticalMarginAlignment="true" OpticalMarginSize="14" />
<ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/Header1">
  <CharacterStyleRange AppliedCharacterStyle="$ID/NormalCharacterStyle">
    <Content>Chapter One</Content>
  </CharacterStyleRange>
</ParagraphStyleRange>
<Br />
<ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/Paragraph">
  <CharacterStyleRange AppliedCharacterStyle="$ID/NormalCharacterStyle">
    <Content>It was a </Content>
  </CharacterStyleRange>
  <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/Italic">
    <Content>dark</Content>
  </CharacterStyleRange>
  <CharacterStyleRange AppliedCharacterStyle="$ID/NormalCharacterStyle">
    <HyperlinkTextSource Self="htss-1" Name="link" Hidden="false"><Content> night</Content></HyperlinkTextSource>
    <Content>.</Content>
  </CharacterStyleRange>
</ParagraphStyleRange>
  </Story>
  <HyperlinkURLDestination Self="HyperlinkURLDestination/1" DestinationURL="https://example.org/" Name="link" DestinationUniqueKey="1" />
  <Hyperlink Self="uf-1" Name="link" Source="htss-1" Visible="false" DestinationUniqueKey="1">
    <Properties><Destination type="object">HyperlinkURLDestination/1</Destination></Properties>
  </Hyperlink>
</Document>"#;

/// `body` (the children of a `Story`) in an ICML file with `resources` before the story and
/// `after` after it.
fn icml(resources: &str, body: &str, after: &str) -> Vec<u8> {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<?aid style="50" type="snippet" readerVersion="6.0" featureSet="513" product="16.0(1)" ?>
<?aid SnippetType="InCopyInterchange"?>
<Document DOMVersion="16.0" Self="d">{resources}<Story Self="main">{body}</Story>{after}</Document>"#
    )
    .into_bytes()
}

/// A paragraph range of `style` holding one character range with `content`.
fn para(style: &str, content: &str) -> String {
    format!(
        r#"<ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/{style}"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]">{content}</CharacterStyleRange></ParagraphStyleRange>"#
    )
}

/// Import without reading links.
fn read(bytes: &[u8]) -> Result<ImportedStory> {
    import_icml(bytes, &|_| None)
}

/// The placed story of an import.
fn story(i: &ImportedStory) -> &Story {
    i.document.stories.get(&i.story).expect("the placed story")
}

/// The text of each character run of `s` with its character style, in order.
fn runs(s: &Story) -> Vec<(String, String)> {
    s.runs().map(|(r, f)| (s.text[r].to_string(), f.style.clone())).collect()
}

#[test]
fn reads_a_converter_style_file() {
    let bytes = PANDOC_STYLE.as_bytes();
    assert!(is_icml(bytes));
    let i = read(bytes).unwrap();
    let s = story(&i);
    assert_eq!(s.text, "Chapter One\nIt was a dark night.");
    assert!(s.frames.is_empty());
    let styles: Vec<&str> = s.paras.iter().map(|p| p.style.as_str()).collect();
    assert_eq!(styles, ["Header1", "Paragraph"]);
    // The italic run is exactly the word; the text keeps the file's "Default" character style,
    // and the return between the paragraphs, outside any range, has none.
    let want = [("Chapter One", "Default"), ("\n", st::NO_CHAR_STYLE), ("It was a ", "Default"), ("dark", "Italic"), (" night.", "Default")];
    assert_eq!(runs(s), want.map(|(t, c)| (t.to_string(), c.to_string())));
    assert_eq!(i.paragraph_styles, ["Header1", "Paragraph"]);
    assert_eq!(i.character_styles, ["Default", "Italic"]);
    let st = &i.document.styles;
    assert_eq!(st.para("Header1").and_then(|p| p.chars.size), Some(36.0));
    assert_eq!(st.para("Header1").and_then(|p| p.based_on.clone()).as_deref(), Some(designcraft_doc::story::BASIC_PARAGRAPH));
    assert_eq!(st.para("Paragraph").and_then(|p| p.para.first_line_indent), Some(12.0));
    assert_eq!(st.char_style("Italic").and_then(|c| c.chars.font_style.clone()).as_deref(), Some("Italic"));
    assert!(s.optical_margin.on && (s.optical_margin.size - 14.0).abs() < 1e-9);
    // The URL isn't kept, and the import says so.
    assert!(i.warnings.iter().any(|w| w.contains("1 hyperlink(s) kept as text")), "{:?}", i.warnings);
}

#[test]
fn sniffs_only_icml() {
    assert!(is_icml(&icml("", "", "")));
    assert!(is_icml(&[b"\xEF\xBB\xBF".as_slice(), &icml("", "", "")].concat()));
    // An IDML designmap and an IDMS snippet are other formats.
    assert!(!is_icml(br#"<?xml version="1.0"?><?aid style="50" type="document"?><Document/>"#));
    assert!(!is_icml(br#"<?xml version="1.0"?><?aid SnippetType="PageItem"?><Document/>"#));
    assert!(!is_icml(b"SnippetType=\"InCopyInterchange\" but plain text"));
    assert!(!is_icml(b""));
}

#[test]
fn refuses_what_is_not_a_story() {
    assert!(matches!(read(b"<Snippet/>"), Err(IdmlError::NotIdml(_))));
    assert!(matches!(read(b"not xml <"), Err(IdmlError::Part { .. }) | Err(IdmlError::NotIdml(_))));
    let none = br#"<?aid SnippetType="InCopyInterchange"?><Document DOMVersion="16.0" Self="d"><Color Self="Color/x"/></Document>"#;
    assert!(matches!(read(none), Err(IdmlError::Invalid(m)) if m.contains("no story")));
    // A story that is its own anchored frame's story is a cycle: refused, not followed.
    let frame = r#"<TextFrame Self="tf" ParentStory="main" ItemTransform="1 0 0 1 0 0"><Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
      <PathPointType Anchor="0 0" LeftDirection="0 0" RightDirection="0 0"/><PathPointType Anchor="0 20" LeftDirection="0 20" RightDirection="0 20"/>
      <PathPointType Anchor="20 20" LeftDirection="20 20" RightDirection="20 20"/><PathPointType Anchor="20 0" LeftDirection="20 0" RightDirection="20 0"/>
      </PathPointArray></GeometryPathType></PathGeometry></Properties></TextFrame>"#;
    let cyclic = icml("", &para("$ID/NormalParagraphStyle", &format!("<Content>a</Content>{frame}")), "");
    assert!(read(&cyclic).is_err());
}

#[test]
fn package_references_are_reported_not_followed() {
    let bytes = icml(r#"<idPkg:Styles src="Resources/Styles.xml"/>"#, &para("$ID/NormalParagraphStyle", "<Content>Text</Content>"), "");
    let i = read(&bytes).unwrap();
    assert_eq!(story(&i).text, "Text");
    assert!(i.warnings.iter().any(|w| w.starts_with("Resources/Styles.xml:")), "{:?}", i.warnings);
}

#[test]
fn places_the_first_story_and_leaves_the_others_out() {
    let mut bytes = icml("", &para("$ID/NormalParagraphStyle", "<Content>First</Content>"), "");
    let second = format!(r#"<Story Self="second">{}</Story></Document>"#, para("$ID/NormalParagraphStyle", "<Content>Second</Content>"));
    bytes.truncate(bytes.len() - "</Document>".len());
    bytes.extend_from_slice(second.as_bytes());
    let i = read(&bytes).unwrap();
    assert_eq!(story(&i).text, "First");
    assert_eq!(i.document.stories.len(), 1);
    assert!(i.warnings.iter().any(|w| w.contains("1 other story")), "{:?}", i.warnings);
}

#[test]
fn styles_the_story_references_but_the_file_lacks_are_not_listed_as_defined() {
    let i = read(&icml("", &para("Missing", "<Content>Text</Content>"), "")).unwrap();
    assert_eq!(story(&i).paras[0].style, "Missing");
    assert!(i.paragraph_styles.is_empty());
}

#[test]
fn the_size_limit_is_checked_before_parsing() {
    // A file over the limit is refused by its length; this one only claims to be (it is short).
    let err = check_size(MAX_ICML_BYTES + 1).unwrap_err();
    assert!(matches!(err, IdmlError::NotIdml(m) if m.contains("at most")));
    assert!(check_size(MAX_ICML_BYTES).is_ok());
}

/// A document whose one story holds what an ICML file carries: styles (one with a next style),
/// a swatch, a footnote, a table, a text anchor with a cross-reference to it, an anchored image
/// and an anchored text frame. Returns it and the story.
fn rich_document() -> (Document, designcraft_doc::StoryId) {
    let mut d = Document::new(&NewDocument::default());
    d.swatches.push(Swatch::color("Brand", Color::rgb(0.8, 0.1, 0.1)));
    {
        let st = d.styles_mut();
        st.paragraph.push(ParagraphStyle {
            name: "Head".into(),
            based_on: Some(designcraft_doc::story::BASIC_PARAGRAPH.into()),
            next_style: Some("Body".into()),
            para: ParaAttrs { space_after: Some(9.0), ..Default::default() },
            chars: CharAttrs { size: Some(20.0), ..Default::default() },
            shortcut: String::new(),
        });
        st.paragraph.push(ParagraphStyle {
            name: "Body".into(),
            based_on: Some(designcraft_doc::story::BASIC_PARAGRAPH.into()),
            next_style: None,
            para: ParaAttrs { first_line_indent: Some(12.0), ..Default::default() },
            chars: CharAttrs::default(),
            shortcut: String::new(),
        });
        st.character.push(CharacterStyle {
            name: "Em".into(),
            based_on: None,
            chars: CharAttrs { font_style: Some("Italic".into()), ..Default::default() },
            shortcut: String::new(),
        });
    }
    let layer = d.default_layer();
    let text = "Heading\nBody with a red word and a note.\nSee here.";
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(40.0, 40.0, 400.0, 600.0), layer, text, ParaFormat::default()).unwrap();
    // The anchored text frame's own story.
    let (inner, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 60.0, 30.0), layer, "Sidebar", ParaFormat::default()).unwrap();
    let inner_item = d.item(inner).unwrap().clone();
    d.spread_mut(SpreadRef::Doc(0)).unwrap().items.retain(|i| i.id != inner);
    let asset = AssetId(d.alloc());
    let asset = d.add_asset(Asset {
        id: asset,
        name: "dot.png".into(),
        mime: "image/png".into(),
        data: Arc::new(PNG.to_vec()),
        pixels: Some((1, 1)),
        ..Default::default()
    });
    let mut image = Item::new(ItemId(d.alloc()), layer, Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0)));
    image.content = Content::Graphic(designcraft_doc::Graphic {
        layer_options: None,
        asset,
        size: (1.0, 1.0),
        xf: designcraft_geom::Affine::scale(10.0),
        clip: None,
        auto_fit: Default::default(),
        fit_align: 4,
        crop: [0.0; 4],
        wrap: Default::default(),
        fill: Default::default(),
    });
    let mut table = Table::new(d.alloc(), 2, 2, 0, 0, 200.0);
    table.cell_mut(1, 1).unwrap().text.insert(0, "Cell");
    let s = d.story_mut(sid).unwrap();
    s.format_paras(0..1, |p| p.style = "Head".into());
    s.format_paras(8..9, |p| p.style = "Body".into());
    let red = s.text.find("red").unwrap();
    s.format_chars(red..red + 3, |f| {
        f.style = "Em".into();
        f.over.fill = Some("Brand".into());
    });
    let note = s.text.find(" note").unwrap() + 5;
    s.insert_note(note, "The note.", ParaFormat::default());
    let target = s.insert_anchor(0, "Top");
    let see = s.text.find("here").unwrap();
    s.insert_xref(see, CrossRef { target, format: "Paragraph Text".into(), ..Default::default() });
    s.insert_object(s.len(), AnchoredObject::new(image, AnchorPosition::Inline { y_offset: 0.0 }));
    s.insert_object(s.len(), AnchoredObject::new(inner_item, AnchorPosition::Inline { y_offset: 1.0 }));
    let body_end = s.text.find('\n').unwrap() + 1;
    s.insert_table(body_end, table);
    d.check().unwrap();
    (d, sid)
}

#[test]
fn a_story_exports_as_one_icml_file() {
    let (d, sid) = rich_document();
    let bytes = export_icml(&d, sid).unwrap();
    let xml = String::from_utf8(bytes.clone()).unwrap();
    assert!(is_icml(&bytes));
    assert!(xml.contains("<?aid style=\"50\" type=\"snippet\""), "{xml}");
    // The story and its anchored frame's story; nothing that belongs to pages.
    assert_eq!(xml.matches("<Story ").count(), 2, "{xml}");
    for absent in ["<Spread", "<MasterSpread", "<Layer ", "<Section ", "idPkg:", "<DocumentPreference"] {
        assert!(!xml.contains(absent), "{absent} in {xml}");
    }
    // Resources come before the story that applies them.
    let story_at = xml.find("<Story ").unwrap();
    for res in ["<Color ", "<RootParagraphStyleGroup", "<RootCharacterStyleGroup", "<CrossReferenceFormat", "<FootnoteOption"] {
        assert!(xml.find(res).is_some_and(|at| at < story_at), "{res} before the story: {xml}");
    }
    assert!(xml.find("<Hyperlink ").is_some_and(|at| at > story_at), "{xml}");
    assert!(matches!(export_icml(&d, designcraft_doc::StoryId(u64::MAX)), Err(IdmlError::Invalid(_))));
}

#[test]
fn an_exported_story_reads_back() {
    let (d, sid) = rich_document();
    let i = read(&export_icml(&d, sid).unwrap()).unwrap();
    let (orig, back) = (d.story(sid).unwrap(), story(&i));
    assert_eq!(back.text, orig.text);
    let styles = |s: &Story| s.paras.iter().map(|p| p.style.clone()).collect::<Vec<_>>();
    assert_eq!(styles(back), styles(orig));
    // The red word: same range, style and colour.
    let red = |s: &Story| s.runs().filter(|(_, f)| f.style == "Em").map(|(r, f)| (r, f.over.fill.clone())).collect::<Vec<_>>();
    assert_eq!(red(back), red(orig));
    assert_eq!(red(back).len(), 1);
    let st = &i.document.styles;
    assert_eq!(st.para("Head").and_then(|p| p.next_style.clone()).as_deref(), Some("Body"));
    assert_eq!(st.para("Head").and_then(|p| p.chars.size), Some(20.0));
    assert_eq!(st.para("Body").and_then(|p| p.para.first_line_indent), Some(12.0));
    assert!(i.paragraph_styles.iter().any(|n| n == "Head") && i.character_styles.iter().any(|n| n == "Em"));
    assert!(i.document.swatches.iter().any(|s| s.name == "Brand"));
    assert_eq!(back.notes.len(), 1);
    assert_eq!(back.notes[0].text.text, "The note.");
    let table = back.tables.values().next().unwrap();
    assert_eq!((table.nrows(), table.ncols()), (2, 2));
    assert_eq!(table.cell(1, 1).unwrap().text.text, "Cell");
    // The cross-reference points at the anchor it pointed at.
    assert_eq!(back.anchors.len(), 1);
    assert_eq!(back.xrefs.len(), 1);
    assert_eq!(back.xrefs[0].target, back.anchors[0].id);
    // The image comes with its data; the anchored frame with its story.
    assert_eq!(back.objects.len(), 2);
    let Content::Graphic(g) = &back.objects[0].item.content else { panic!("an image first: {:?}", back.objects[0].item.content) };
    assert_eq!(i.document.assets.get(&g.asset).map(|a| a.data.as_slice()), Some(PNG));
    let tf = back.objects[1].item.text_frame().unwrap();
    let inner = i.document.stories.get(&tf.story).unwrap();
    assert_eq!(inner.text, "Sidebar");
    assert_eq!(inner.frames, vec![back.objects[1].item.id]);
    i.document.check().unwrap();
}
