use super::*;

const NS: &str = r#"xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="14.0""#;

/// A page with one text frame at its top edge whose custom baseline grid starts at 6 pt with a
/// 14 pt increment; its four paragraphs align to the grid. The frame's top is the page's top, so
/// the grid lines are 6 + n·14 pt down from the frame top whatever the grid is measured from.
fn frame_grid_doc() -> Document {
    frame_grid_doc_relative(None)
}

/// [`frame_grid_doc`] with `BaselineFrameGridRelativeOption` set to `relative` when given.
fn frame_grid_doc_relative(relative: Option<&str>) -> Document {
    let relative = relative.map(|r| format!(r#" BaselineFrameGridRelativeOption="{r}""#)).unwrap_or_default();
    let designmap = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Document {NS} Self="d" StoryList="s1" Name="grid.indd"><idPkg:Graphic src="Resources/Graphic.xml"/><idPkg:Styles src="Resources/Styles.xml"/><idPkg:Preferences src="Resources/Preferences.xml"/><Layer Self="L1" Name="Layer 1" Visible="true" Locked="false"/><idPkg:Spread src="Spreads/Spread_sp1.xml"/><idPkg:Story src="Stories/Story_s1.xml"/></Document>"#
    );
    let prefs = format!(r#"<idPkg:Preferences {NS}><DocumentPreference PageWidth="400" PageHeight="600" FacingPages="false"/></idPkg:Preferences>"#);
    let pts: String = [(50.0, 0.0), (50.0, 300.0), (250.0, 300.0), (250.0, 0.0)]
        .iter()
        .map(|(x, y)| format!(r#"<PathPointType Anchor="{x} {y}" LeftDirection="{x} {y}" RightDirection="{x} {y}"/>"#))
        .collect();
    let spread = format!(
        r#"<idPkg:Spread {NS}><Spread Self="sp1" PageCount="1" ItemTransform="1 0 0 1 0 0"><Page Self="p1" GeometricBounds="0 0 600 400" ItemTransform="1 0 0 1 0 -300"/>
<TextFrame Self="t1" ParentStory="s1" PreviousTextFrame="n" NextTextFrame="n" ItemLayer="L1" ItemTransform="1 0 0 1 0 -300"><Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>{pts}</PathPointArray></GeometryPathType></PathGeometry></Properties><TextFramePreference InsetSpacing="0"/><BaselineFrameGridOption UseCustomBaselineFrameGrid="true" StartingOffsetForBaselineFrameGrid="6" BaselineFrameGridIncrement="14"{relative}/></TextFrame>
</Spread></idPkg:Spread>"#
    );
    let story = format!(
        r#"<idPkg:Story {NS}><Story Self="s1"><ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/[No paragraph style]" GridAlignment="AlignToBaseline"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>One</Content><Br/><Content>Two</Content><Br/><Content>Three</Content><Br/><Content>Four</Content></CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>"#
    );
    let bytes = zip_files(&[
        ("designmap.xml", &designmap),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Styles.xml", STYLES),
        ("Resources/Preferences.xml", &prefs),
        ("Spreads/Spread_sp1.xml", &spread),
        ("Stories/Story_s1.xml", &story),
    ]);
    import_idml_with(&bytes, &|_| None).unwrap()
}

/// Each composed line's baseline below the frame top.
fn baselines(d: &Document) -> Vec<f64> {
    let sid = *d.stories.keys().next().unwrap();
    let top = d.spreads[0].items[0].text_area().y0;
    let cs = designcraft_compose::compose_story(d, sid, &Default::default());
    cs.frames.iter().flat_map(|f| f.lines.iter()).map(|l| l.baseline - top).collect()
}

#[test]
fn custom_frame_grid_sets_lines_from_its_start_by_its_increment() {
    let d = frame_grid_doc();
    let grid = d.spreads[0].items[0].text_frame().unwrap().options.baseline_grid;
    assert_eq!(grid, Some(designcraft_doc::FrameBaselineGrid::new(6.0, 14.0)));
    let b = baselines(&d);
    assert_eq!(b.len(), 4, "{b:?}");
    for y in &b {
        let n = (y - 6.0) / 14.0;
        assert!((n - n.round()).abs() < 1e-6 && n >= 0.0, "baseline {y} is on the 6 + n·14 grid: {b:?}");
    }
    // The first line's ascent is past the 6 pt line, so it sits on the next one.
    assert!((b[0] - 20.0).abs() < 1e-6, "{b:?}");
}

#[test]
fn custom_frame_grid_round_trips() {
    let d = frame_grid_doc();
    let bytes = export_idml(&d);
    let spread = zip_text(&bytes, "Spreads/");
    assert!(spread.contains(r#"StartingOffsetForBaselineFrameGrid="6""#), "{spread}");
    assert!(spread.contains(r#"BaselineFrameGridIncrement="14""#), "{spread}");
    let back = import_idml(&bytes).unwrap();
    let grid = |d: &Document| d.spreads[0].items[0].text_frame().unwrap().options.baseline_grid;
    assert_eq!(grid(&back), grid(&d));
    assert_eq!(baselines(&back), baselines(&d));
}

#[test]
fn custom_frame_grid_relative_to_round_trips() {
    use designcraft_doc::FrameGridRelative as R;
    let relative = |d: &Document| d.spreads[0].items[0].text_frame().unwrap().options.baseline_grid.map(|g| g.relative_to);
    for (attr, want) in [
        ("TopOfPageOfBaselineFrameGridRelativeOption", R::TopOfPage),
        ("TopOfMarginOfBaselineFrameGridRelativeOption", R::TopMargin),
        ("TopOfFrameOfBaselineFrameGridRelativeOption", R::TopOfFrame),
        ("TopOfInsetOfBaselineFrameGridRelativeOption", R::TopInset),
        ("TopOfInset", R::TopInset),
        ("SomethingNew", R::TopOfPage),
    ] {
        let d = frame_grid_doc_relative(Some(attr));
        assert_eq!(relative(&d), Some(want), "{attr}");
        let back = import_idml(&export_idml(&d)).unwrap();
        assert_eq!(relative(&back), Some(want), "{attr} after export");
    }
    // A frame without the attribute measures from the top of the page.
    assert_eq!(relative(&frame_grid_doc()), Some(R::TopOfPage));
}
