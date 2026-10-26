use designcraft_doc::vars::{Use, VarKind, format_date, now, var_char};

use super::*;

const VARIABLES: &str = r#"
  <TextVariable Self="dTextVariablen&lt;?AID 001b?&gt;TV XRefPageNumber" Name="&lt;?AID 001b?&gt;TV XRefPageNumber" VariableType="XrefPageNumberType" />
  <TextVariable Self="dTextVariablenCreation Date" Name="Creation Date" VariableType="CreationDateType">
    <DateVariablePreference TextBefore="made " Format="MM/dd/yy" TextAfter="" />
  </TextVariable>
  <TextVariable Self="dTextVariablenImage Name" Name="Image Name" VariableType="LiveCaptionType">
    <CaptionMetadataVariablePreference TextBefore="" MetadataProviderName="$ID/#LinkInfoNameStr" TextAfter="" />
  </TextVariable>
  <TextVariable Self="dTextVariablenOutput Date" Name="Output Date" VariableType="OutputDateType">
    <DateVariablePreference TextBefore="" Format="MM/dd/yy" TextAfter="" />
  </TextVariable>
  <TextVariable Self="dTextVariablenEdition" Name="Edition" VariableType="CustomTextType">
    <CustomTextVariablePreference Contents="First edition" />
  </TextVariable>
  <TextVariable Self="dTextVariablenLast Page Number" Name="Last Page Number" VariableType="LastPageNumberType">
    <PageNumberVariablePreference TextBefore="of " Format="Current" TextAfter="" Scope="SectionScope" />
  </TextVariable>
  <TextVariable Self="dTextVariablenRunning Header" Name="Running Header" VariableType="MatchParagraphStyleType">
    <MatchParagraphStylePreference TextBefore="" TextAfter="" AppliedParagraphStyle="ParagraphStyle/Text%3aBody" SearchStrategy="LastOnPage" ChangeCase="None" DeleteEndPunctuation="false" />
  </TextVariable>
  <idPkg:Story"#;

const STORY: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <Story Self="s1">
    <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/NormalParagraphStyle">
      <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>output </Content></CharacterStyleRange>
      <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" PageNumberType="TextVariable">
        <TextVariableInstance Self="u1" Name="Output Date" ResultText="12/16/19" AssociatedTextVariable="dTextVariablenOutput Date" />
      </CharacterStyleRange>
      <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]">
        <Content> / </Content>
        <TextVariableInstance Self="u2" Name="Edition" ResultText="Old edition" AssociatedTextVariable="dTextVariablenEdition" />
        <Content> / </Content>
        <TextVariableInstance Self="u3" Name="Creation Date" ResultText="made 01/01/01" AssociatedTextVariable="dTextVariablenCreation Date" />
        <Content> / </Content>
        <TextVariableInstance Self="u4" Name="Image Name" ResultText="photo.jpg" AssociatedTextVariable="dTextVariablenImage Name" />
        <Content> / </Content>
        <TextVariableInstance Self="u5" Name="Gone" ResultText="kept" AssociatedTextVariable="dTextVariablenGone" />
      </CharacterStyleRange>
    </ParagraphStyleRange>
  </Story>
</idPkg:Story>"#;

const XMP: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/">
    <xmp:CreateDate>2019-12-16T09:48:02+01:00</xmp:CreateDate>
    <xmp:ModifyDate>2019-12-16T10:23:18+01:00</xmp:ModifyDate>
  </rdf:Description></rdf:RDF></x:xmpmeta>"#;

/// Glyph ids of the story's first frame, and those of its spaces.
fn glyphs(d: &Document) -> (Vec<u32>, Vec<u32>) {
    let sid = *d.stories.keys().next().unwrap();
    let cs = designcraft_compose::compose_story(d, sid, &Default::default());
    let text = &d.stories[&sid].text;
    let all: Vec<_> = cs.frames[0].lines.iter().flat_map(|l| l.glyphs.iter()).collect();
    (all.iter().map(|g| g.gid).collect(), all.iter().filter(|g| text[g.byte..].starts_with(' ')).map(|g| g.gid).collect())
}

/// The story's glyphs and those of `text` set as plain text in its place, without spaces (a
/// variable doesn't break across lines, so line-end spaces fall differently).
fn shown(d: &Document, text: &str) -> (Vec<u32>, Vec<u32>) {
    let mut plain = d.clone();
    let sid = *plain.stories.keys().next().unwrap();
    let st = plain.story_mut(sid).unwrap();
    let n = st.text.len();
    st.replace(0..n, text);
    let (want, spaces) = glyphs(&plain);
    let got = glyphs(d).0;
    let drop = |v: Vec<u32>| v.into_iter().filter(|g| !spaces.contains(g)).collect();
    (drop(got), drop(want))
}

#[test]
fn text_variables_show_their_values_and_round_trip() {
    let map = DESIGNMAP.replacen("\n  <idPkg:Story", VARIABLES, 1);
    let bytes = zip_files(&[
        ("designmap.xml", &map),
        ("META-INF/metadata.xml", XMP),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Styles.xml", STYLES),
        ("Resources/Preferences.xml", PREFS),
        ("MasterSpreads/MasterSpread_m1.xml", MASTER),
        ("Spreads/Spread_sp1.xml", SPREAD),
        ("Stories/Story_s1.xml", STORY),
    ]);
    let d = import_idml_with(&bytes, &|_| None).unwrap();
    // The file's variables replace the defaults; cross-reference internals are left out.
    let names: Vec<&str> = d.text_variables.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["Creation Date", "Image Name", "Output Date", "Edition", "Last Page Number", "Running Header"]);
    assert_eq!(d.text_variables[4].kind, VarKind::LastPageNumber { section: true });
    assert_eq!(d.text_variables[4].before, "of ");
    assert_eq!(d.text_variables[5].kind, VarKind::RunningHeader { style: "Text/Body".into(), use_: Use::LastOnPage, character: false });
    let v = |n: &str| var_char(d.variable(n).unwrap()).unwrap();
    let story = d.stories.values().next().unwrap();
    assert_eq!(story.text, format!("output {} / {} / {} / {} / kept", v("Output Date"), v("Edition"), v("Creation Date"), v("Image Name")));
    assert!(matches!(&d.text_variables[1].kind, VarKind::Imported { result, .. } if result == "photo.jpg"));

    // Output date is today; creation date comes from the package metadata; the caption shows
    // its recorded result; a custom variable shows its contents.
    let today = format_date(now(), "MM/dd/yy");
    let expected = format!("output {today} / First edition / made 12/16/19 / photo.jpg / kept");
    let (got, want) = shown(&d, &expected);
    if today == format_date(now(), "MM/dd/yy") {
        assert_eq!(got, want);
    }

    let back = import_idml(&export_idml(&d)).unwrap();
    back.check().unwrap();
    assert_eq!(back.text_variables, d.text_variables);
    assert_eq!(back.stories.values().next().unwrap().text, story.text);
    assert_eq!((back.created, back.modified), (d.created, d.modified));
    assert_eq!(glyphs(&back).0, glyphs(&d).0);
}

#[test]
fn predefined_variables_round_trip() {
    let mut d = Document::new(&NewDocument::default());
    d.text_variables.push(designcraft_doc::vars::TextVariable::new(
        "Chapter Head",
        VarKind::RunningHeader { style: "Strong".into(), use_: Use::FirstOnPage, character: true },
    ));
    let back = import_idml(&export_idml(&d)).unwrap();
    assert_eq!(back.text_variables, d.text_variables);
}
