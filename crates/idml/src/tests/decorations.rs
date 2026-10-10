use super::*;

fn fixture(local: &str) -> Document {
    let styles = r#"<idPkg:Styles xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
      <RootParagraphStyleGroup>
        <ParagraphStyle Self="ParagraphStyle/PBase" Name="PBase" Underline="true" StrikeThru="true" UnderlineColor="Color/Black" StrikeThruColor="Color/Black"/>
        <ParagraphStyle Self="ParagraphStyle/PReset" Name="PReset" UnderlineColor=" Text Color " StrikeThruColor="Text Color"><Properties><BasedOn type="object">ParagraphStyle/PBase</BasedOn></Properties></ParagraphStyle>
        <ParagraphStyle Self="ParagraphStyle/PLeaf" Name="PLeaf"><Properties><BasedOn type="object">ParagraphStyle/PReset</BasedOn></Properties></ParagraphStyle>
      </RootParagraphStyleGroup>
      <RootCharacterStyleGroup>
        <CharacterStyle Self="CharacterStyle/CBase" Name="CBase" UnderlineColor="Color/Black" StrikeThruColor="Color/Black"/>
        <CharacterStyle Self="CharacterStyle/CReset" Name="CReset"><Properties><BasedOn type="object">CharacterStyle/CBase</BasedOn><UnderlineColor type="string">Text Color</UnderlineColor><StrikeThruColor type="string">Text Color</StrikeThruColor></Properties></CharacterStyle>
        <CharacterStyle Self="CharacterStyle/CLeaf" Name="CLeaf"><Properties><BasedOn type="object">CharacterStyle/CReset</BasedOn></Properties></CharacterStyle>
        <CharacterStyle Self="CharacterStyle/CInherit" Name="CInherit"><Properties><BasedOn type="object">CharacterStyle/CBase</BasedOn></Properties></CharacterStyle>
      </RootCharacterStyleGroup>
    </idPkg:Styles>"#;
    let story = format!(
        r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Story Self="s"><ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/PBase"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/CBase" {local}><Content>Ink</Content></CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>"#
    );
    let map = r#"<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><idPkg:Styles src="Resources/Styles.xml"/><idPkg:Story src="Stories/s.xml"/><Layer Self="L" Name="Art"/><idPkg:Spread src="Spreads/s.xml"/></Document>"#;
    let spread = r#"<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Spread Self="sp"><Page Self="p" GeometricBounds="0 0 240 240"/><TextFrame Self="frame" ParentStory="s" ItemLayer="L" GeometricBounds="0 0 200 200"><Properties><PathGeometry><GeometryPathType><PathPointArray><PathPointType Anchor="0 0"/><PathPointType Anchor="200 0"/><PathPointType Anchor="200 200"/><PathPointType Anchor="0 200"/></PathPointArray></GeometryPathType></PathGeometry></Properties></TextFrame></Spread></idPkg:Spread>"#;
    import_idml(&zip_files(&[("designmap.xml", map), ("Resources/Styles.xml", styles), ("Stories/s.xml", &story), ("Spreads/s.xml", spread)]))
        .unwrap()
}

fn assert_style_resets(d: &Document) {
    let p = d.styles.para("PReset").unwrap();
    let c = d.styles.char_style("CReset").unwrap();
    for attrs in [&p.chars, &c.chars] {
        assert_eq!(attrs.underline_color.as_deref(), Some(""), "an explicit reset must remain a sparse value");
        assert_eq!(attrs.strikethrough_color.as_deref(), Some(""));
    }
    let (_, base) = d.styles.resolve_para_style("PBase");
    let (_, reset) = d.styles.resolve_para_style("PLeaf");
    assert_eq!(base.underline_color, "[Black]");
    assert_eq!(base.strikethrough_color, "[Black]");
    assert!(reset.underline_color.is_empty() && reset.strikethrough_color.is_empty());
    let reset = d.styles.resolve_char(&base, &designcraft_doc::CharFormat { style: "CLeaf".into(), ..Default::default() });
    assert!(reset.underline_color.is_empty() && reset.strikethrough_color.is_empty());
    let inherited = d.styles.resolve_char(&base, &designcraft_doc::CharFormat { style: "CInherit".into(), ..Default::default() });
    assert_eq!(inherited.underline_color, "[Black]");
    assert_eq!(inherited.strikethrough_color, "[Black]");
    assert!(d.styles.char_style("CInherit").unwrap().chars.underline_color.is_none());
}

#[test]
fn text_color_resets_paragraph_and_character_style_chains_through_roundtrip() {
    let d = fixture("");
    assert_style_resets(&d);
    assert_style_resets(&import_idml(&export_idml(&d)).unwrap());
}

#[test]
fn local_text_color_and_explicit_none_remain_distinct_through_roundtrip() {
    for (local, expected) in [
        (r#"UnderlineColor="Text Color" StrikeThruColor="Text Color""#, ""),
        (r#"UnderlineColor="n" StrikeThruColor="n""#, "[None]"),
        (r#"UnderlineColor="" StrikeThruColor="""#, "[None]"),
        ("", "[Black]"),
    ] {
        let imported = fixture(local);
        let reopened = import_idml(&export_idml(&imported)).unwrap();
        for d in [&imported, &reopened] {
            let story = d.stories.values().next().unwrap();
            let (_, para) = d.styles.resolve_para(&story.paras[0]);
            let actual = d.styles.resolve_char(&para, story.char_format_at(0));
            assert_eq!(actual.underline_color, expected);
            assert_eq!(actual.strikethrough_color, expected);
        }
    }
}

#[test]
fn exporting_native_text_color_reset_writes_the_sentinel() {
    let mut d = fixture("");
    let attrs = &mut d.styles_mut().para_mut("PReset").unwrap().chars;
    attrs.underline_color = Some(String::new());
    attrs.strikethrough_color = Some(String::new());
    let attrs = &mut d.styles_mut().char_style_mut("CReset").unwrap().chars;
    attrs.underline_color = Some(String::new());
    attrs.strikethrough_color = Some(String::new());
    assert_style_resets(&import_idml(&export_idml(&d)).unwrap());
}
