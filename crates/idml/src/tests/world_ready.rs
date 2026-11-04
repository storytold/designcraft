use super::*;

/// Synthetic Tamil text with inherited styles and a local override in either XML representation.
fn fixture(composer: &str, local_composer: &str, property: bool) -> Vec<u8> {
    let composer_xml = |name: &str| {
        if property {
            (String::new(), format!(r#"<Composer type="string">$ID/{name}</Composer>"#))
        } else {
            (format!(r#" Composer="{name}""#), String::new())
        }
    };
    let (attr, prop) = composer_xml(composer);
    let styles = format!(
        r#"<idPkg:Styles xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
        <RootParagraphStyleGroup Self="rp">
          <ParagraphStyle Self="ParagraphStyle/$ID/[No paragraph style]" Name="$ID/[No paragraph style]"/>
          <ParagraphStyle Self="ParagraphStyle/WorldReady" Name="WorldReady"{attr}>
            <Properties>{prop}<BasedOn type="string">$ID/[No paragraph style]</BasedOn></Properties>
          </ParagraphStyle>
          <ParagraphStyle Self="ParagraphStyle/Inherited" Name="Inherited">
            <Properties><BasedOn type="object">ParagraphStyle/WorldReady</BasedOn></Properties>
          </ParagraphStyle>
        </RootParagraphStyleGroup></idPkg:Styles>"#
    );
    let (attr, prop) = composer_xml(local_composer);
    let story = format!(
        r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Story Self="s1">
          <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/Inherited">
            <CharacterStyleRange><Content>தமிழ்</Content><Br/></CharacterStyleRange>
          </ParagraphStyleRange>
          <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/Inherited"{attr}>
            <Properties>{prop}</Properties><CharacterStyleRange><Content>தமிழ்</Content></CharacterStyleRange>
          </ParagraphStyleRange>
        </Story></idPkg:Story>"#
    );
    zip_files(&[
        ("designmap.xml", DESIGNMAP),
        ("Resources/Styles.xml", &styles),
        ("Resources/Preferences.xml", PREFS),
        ("MasterSpreads/MasterSpread_m1.xml", MASTER),
        ("Spreads/Spread_sp1.xml", SPREAD),
        ("Stories/Story_s1.xml", &story),
    ])
}

/// #2032: both the paragraph style and the local override must retain their vendor identifier.
/// Inspect the exported XML too: an importer and exporter agreeing on the wrong value is not proof.
#[test]
fn world_ready_composer_identifiers_survive_idml_export() {
    for composer in ["HL Composer Optyca", "HL Single Optyca"] {
        for property in [false, true] {
            let doc = import_idml(&fixture(composer, composer, property)).unwrap();
            let bytes = export_idml(&doc);
            let expected = format!(r#"Composer="{composer}""#);
            assert!(zip_text(&bytes, "Resources/Styles.xml").contains(&expected), "style: {composer}, property={property}");
            assert!(zip_text(&bytes, "Stories/").contains(&expected), "override: {composer}, property={property}");
        }
    }
}

/// Import/export preserves style inheritance, explicit overrides and Tamil story text.
#[test]
fn world_ready_composer_styles_inheritance_and_overrides_round_trip() {
    use designcraft_doc::Composer;

    for (name, composer, local_name, local_composer) in [
        ("HL Composer Optyca", Composer::Paragraph, "HL Single Optyca", Composer::SingleLine),
        ("HL Single Optyca", Composer::SingleLine, "HL Composer Optyca", Composer::Paragraph),
    ] {
        for property in [false, true] {
            let doc = import_idml(&fixture(name, local_name, property)).unwrap();
            let reopened = import_idml(&export_idml(&doc)).unwrap();
            for d in [&doc, &reopened] {
                assert_eq!(d.styles.para("WorldReady").unwrap().para.composer, Some(composer));
                assert_eq!(d.styles.para("WorldReady").unwrap().para.world_ready, Some(true));
                assert_eq!(d.styles.para("Inherited").unwrap().para.composer, None);
                assert_eq!(d.styles.resolve_para_style("Inherited").0.composer, composer);
                let story = d.stories.values().find(|s| s.text.contains("தமிழ்")).unwrap();
                assert_eq!(story.text, "தமிழ்\nதமிழ்");
                assert_eq!(story.paras[0].para.composer, None);
                assert_eq!(story.paras[0].para.world_ready, None);
                assert_eq!(story.paras[1].para.composer, Some(local_composer));
                assert_eq!(d.styles.resolve_para(&story.paras[0]).0.composer, composer);
                assert_eq!(d.styles.resolve_para(&story.paras[1]).0.composer, local_composer);
                assert!(d.styles.resolve_para(&story.paras[0]).0.world_ready);
                assert!(d.styles.resolve_para(&story.paras[1]).0.world_ready);
            }
        }
    }
}

/// IDML combines two attributes in one name: exporting a mode-only or World-Ready-only edit must
/// inherit the other half from its actual parent, for both styles and local overrides.
#[test]
fn world_ready_partial_overrides_resolve_the_other_half_from_the_style() {
    use designcraft_doc::{Composer, ParaAttrs};
    for (name, mode, other, ordinary) in [
        ("HL Composer Optyca", Composer::Paragraph, Composer::SingleLine, "HL Composer"),
        ("HL Single Optyca", Composer::SingleLine, Composer::Paragraph, "HL Single"),
    ] {
        let mut doc = import_idml(&fixture(name, name, false)).unwrap();
        doc.styles_mut().para_mut("Inherited").unwrap().para.world_ready = Some(false);
        let sid = *doc.stories.keys().next().unwrap();
        let story = doc.story_mut(sid).unwrap();
        story.paras[0].para = ParaAttrs { composer: Some(other), ..Default::default() };
        story.paras[1].para = ParaAttrs { world_ready: Some(true), ..Default::default() };
        let bytes = export_idml(&doc);
        let styles = zip_text(&bytes, "Resources/Styles.xml");
        let tag = styles.split("<ParagraphStyle ").filter_map(|tag| tag.split('>').next()).find(|tag| tag.contains(r#"Name="Inherited""#)).unwrap();
        assert!(tag.contains(&format!(r#"Composer="{ordinary}""#)), "{tag}");
        let reopened = import_idml(&bytes).unwrap();
        for doc in [&doc, &reopened] {
            let inherited = doc.styles.resolve_para_style("Inherited").0;
            assert_eq!((inherited.composer, inherited.world_ready), (mode, false));
            let story = doc.story(sid).or_else(|| doc.stories.values().next().map(|s| s.as_ref())).unwrap();
            let first = doc.styles.resolve_para(&story.paras[0]).0;
            let second = doc.styles.resolve_para(&story.paras[1]).0;
            assert_eq!((first.composer, first.world_ready), (other, false));
            assert_eq!((second.composer, second.world_ready), (mode, true));
        }
    }
}
