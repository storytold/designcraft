use super::*;

const NS: &str = r#"xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="14.0""#;

fn wrap_pref(mode: &str, offset: f64) -> String {
    format!(
        r#"<TextWrapPreference Inverse="false" ApplyToMasterPageOnly="false" TextWrapSide="BothSides" TextWrapMode="{mode}"><Properties><TextWrapOffset Top="{offset}" Left="{offset}" Bottom="{offset}" Right="{offset}"/></Properties></TextWrapPreference>"#
    )
}

fn rect_path(x0: f64, y0: f64, x1: f64, y1: f64) -> String {
    let pts: String = [(x0, y0), (x0, y1), (x1, y1), (x1, y0)]
        .iter()
        .map(|(x, y)| format!(r#"<PathPointType Anchor="{x} {y}" LeftDirection="{x} {y}" RightDirection="{x} {y}"/>"#))
        .collect();
    format!(
        r#"<Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>{pts}</PathPointArray></GeometryPathType></PathGeometry></Properties>"#
    )
}

/// A page with a text frame (50, 60)–(370, 540) and a 100 pt square image frame at (160, 150)
/// over it; the image (60 px scaled to fill the frame) and its frame carry the given wraps.
fn wrap_doc(frame_wrap: &str, image_wrap: &str) -> Document {
    let lorem =
        "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. ".repeat(12);
    let designmap = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Document {NS} Self="d" StoryList="s1" Name="wrap.indd"><idPkg:Graphic src="Resources/Graphic.xml"/><idPkg:Styles src="Resources/Styles.xml"/><idPkg:Preferences src="Resources/Preferences.xml"/><Layer Self="L1" Name="Layer 1" Visible="true" Locked="false"/><idPkg:Spread src="Spreads/Spread_sp1.xml"/><idPkg:Story src="Stories/Story_s1.xml"/></Document>"#
    );
    let prefs = format!(r#"<idPkg:Preferences {NS}><DocumentPreference PageWidth="400" PageHeight="600" FacingPages="false"/></idPkg:Preferences>"#);
    let spread = format!(
        r#"<idPkg:Spread {NS}><Spread Self="sp1" PageCount="1" ItemTransform="1 0 0 1 0 0"><Page Self="p1" GeometricBounds="0 0 600 400" ItemTransform="1 0 0 1 0 -300"/>
<TextFrame Self="t1" ParentStory="s1" PreviousTextFrame="n" NextTextFrame="n" ItemLayer="L1" ItemTransform="1 0 0 1 0 -300">{}<TextFramePreference FirstBaselineOffset="LeadingOffset"/></TextFrame>
<Rectangle Self="r1" ItemLayer="L1" ContentType="GraphicType" ItemTransform="1 0 0 1 0 -300">{}{frame_wrap}<Image Self="im1" ItemTransform="1.6667 0 0 1.6667 160 150"><Properties><GraphicBounds Left="0" Top="0" Right="60" Bottom="60"/></Properties>{image_wrap}<Link Self="lk1" LinkResourceURI="file:C:/elsewhere/square.png" LinkResourceFormat="$ID/Portable Network Graphics (PNG)" StoredState="Normal"/></Image></Rectangle>
</Spread></idPkg:Spread>"#,
        rect_path(50.0, 60.0, 370.0, 540.0),
        rect_path(160.0, 150.0, 260.0, 250.0)
    );
    let story = format!(
        r#"<idPkg:Story {NS}><Story Self="s1"><ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/[No paragraph style]"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>{lorem}</Content></CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>"#
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

/// Each composed line as (x0, x1, baseline), rounded to 0.01 pt.
fn lines(d: &Document) -> Vec<(i64, i64, i64)> {
    let sid = *d.stories.keys().next().unwrap();
    let cs = designcraft_compose::compose_story(d, sid, &Default::default());
    let r = |v: f64| (v * 100.0).round() as i64;
    cs.frames.iter().flat_map(|f| f.lines.iter()).map(|l| (r(l.x0), r(l.x1), r(l.baseline))).collect()
}

#[test]
fn wrap_on_a_placed_graphic_wraps_text_like_a_wrap_on_its_frame() {
    let none = wrap_pref("None", 0.0);
    let bbox = wrap_pref("BoundingBoxTextWrap", 6.0);
    let on_image = wrap_doc(&none, &bbox);
    let on_frame = wrap_doc(&bbox, &none);
    let unwrapped = wrap_doc(&none, &none);
    let frame = &on_image.spreads[0].items[1];
    assert_eq!(frame.wrap.mode, designcraft_doc::WrapMode::None);
    let g = frame.graphic().unwrap();
    assert_eq!((g.wrap.mode, g.wrap.offsets), (designcraft_doc::WrapMode::BoundingBox, [6.0; 4]));
    // The image fills its frame, so both wraps make the same obstacle.
    let wrapped = lines(&on_image);
    assert_eq!(wrapped, lines(&on_frame));
    let plain = lines(&unwrapped);
    assert_ne!(wrapped, plain);
    let full = plain[0].1 - plain[0].0;
    assert!(wrapped.iter().any(|l| l.1 - l.0 < full / 2), "some lines run beside the image: {wrapped:?}");
}

#[test]
fn wrap_on_a_placed_graphic_follows_its_crop_and_round_trips() {
    let none = wrap_pref("None", 0.0);
    let d = wrap_doc(&none, &wrap_pref("BoundingBoxTextWrap", 6.0));
    // Exported on the image element, not on the frame.
    let back = import_idml(&export_idml(&d)).unwrap();
    let f = back.spreads[0].items.iter().find(|i| i.graphic().is_some()).unwrap();
    assert_eq!(f.wrap.mode, designcraft_doc::WrapMode::None);
    assert_eq!(f.graphic().unwrap().wrap, d.spreads[0].items[1].graphic().unwrap().wrap);
    // Moved half out of the bottom of its frame, the image wraps only the part the frame shows.
    let mut moved = d.clone();
    let sp = std::sync::Arc::make_mut(&mut moved.spreads[0]);
    let fr = std::sync::Arc::make_mut(&mut sp.items[1]);
    if let designcraft_doc::Content::Graphic(g) = &mut fr.content {
        g.xf = designcraft_geom::Affine::translate((0.0, 50.0)) * g.xf;
    }
    let fr = &moved.spreads[0].items[1];
    let g = fr.graphic().unwrap();
    let image = fr.xf.transform_rect_bbox(g.xf.transform_rect_bbox(designcraft_geom::Rect::new(0.0, 0.0, g.size.0, g.size.1)));
    // In the text frame's inner space, where lines are.
    let inv = moved.spreads[0].items[0].xf.inverse();
    let shown = inv.transform_rect_bbox(fr.bounds().intersect(image));
    let image = inv.transform_rect_bbox(image);
    let sid = *moved.stories.keys().next().unwrap();
    let cs = designcraft_compose::compose_story(&moved, sid, &Default::default());
    let beside: Vec<_> = cs.frames[0].lines.iter().filter(|l| l.baseline > shown.y0 && l.baseline - l.ascent < shown.y1).collect();
    assert!(!beside.is_empty());
    for l in &beside {
        assert!(l.x1 <= shown.x0 - 6.0 + 0.01 || l.x0 >= shown.x1 + 6.0 - 0.01, "{:?} vs {shown:?}", (l.x0, l.x1));
    }
    // Full lines resume below the shown part (plus the offset), not below the whole image.
    let full = cs.frames[0].lines.iter().find(|l| l.baseline - l.ascent > shown.y1 + 6.0).unwrap();
    assert!(full.x1 - full.x0 > 300.0 && full.baseline < image.y1, "{:?} vs {image:?}", (full.x0, full.x1, full.baseline));
}
