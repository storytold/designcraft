//! Page items take the values their object style chain gives wherever the item has none.

use designcraft_geom::corners::CornerShape;

use super::*;

const NONE_STYLE: &str = r#"<ObjectStyle Self="ObjectStyle/$ID/[None]" Name="$ID/[None]" CornerRadius="12" CornerOption="None"
      TopLeftCornerOption="None" TopRightCornerOption="None" BottomLeftCornerOption="None" BottomRightCornerOption="None"
      TopLeftCornerRadius="12" TopRightCornerRadius="12" BottomLeftCornerRadius="12" BottomRightCornerRadius="12">
      <TransparencySetting><DropShadowSetting Mode="None" Opacity="75" XOffset="7" YOffset="7" Size="5" Spread="0"/></TransparencySetting>
      <TextWrapPreference TextWrapMode="None"><Properties><TextWrapOffset Top="0" Left="0" Bottom="0" Right="0"/></Properties></TextWrapPreference>
    </ObjectStyle>"#;

/// A document with `styles` after `[None]` and one 100 pt square rectangle per entry of `items`
/// (attributes, children).
fn fixture(styles: &str, items: &[(&str, &str)]) -> Document {
    let designmap = format!(
        r#"<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" Self="d">
          <RootObjectStyleGroup Self="ro">{NONE_STYLE}{styles}</RootObjectStyleGroup>
          <idPkg:Spread src="Spreads/Spread_s.xml"/>
        </Document>"#
    );
    let rects: String = items
        .iter()
        .enumerate()
        .map(|(i, (attrs, kids))| {
            format!(
                r#"<Rectangle Self="r{i}" {attrs}>
                  <Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
                    <PathPointType Anchor="0 0"/><PathPointType Anchor="0 100"/>
                    <PathPointType Anchor="100 100"/><PathPointType Anchor="100 0"/>
                  </PathPointArray></GeometryPathType></PathGeometry></Properties>
                  {kids}
                </Rectangle>"#
            )
        })
        .collect();
    let spread = format!(
        r#"<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
          <Spread Self="s"><Page Self="p" GeometricBounds="0 0 200 200" ItemTransform="1 0 0 1 0 0"/>{rects}</Spread>
        </idPkg:Spread>"#
    );
    import_idml(&zip_files(&[("designmap.xml", &designmap), ("Spreads/Spread_s.xml", &spread)])).unwrap()
}

fn assert_round(it: &designcraft_doc::Item, size: f64) {
    for c in it.corners.corners {
        assert_eq!(c.shape, CornerShape::Rounded, "{:?}", it.corners);
        assert!((c.size - size).abs() < 1e-9, "{:?}", it.corners);
    }
}

const ROUND: &str = r#"<ObjectStyle Self="ObjectStyle/Round" Name="Round"
      TopLeftCornerOption="RoundedCorner" TopRightCornerOption="RoundedCorner" BottomLeftCornerOption="RoundedCorner" BottomRightCornerOption="RoundedCorner"
      TopLeftCornerRadius="17" TopRightCornerRadius="17" BottomLeftCornerRadius="17" BottomRightCornerRadius="17" CornerRadius="17">
      <Properties><BasedOn type="string">$ID/[None]</BasedOn></Properties>
      <TransparencySetting><DropShadowSetting Mode="Drop" Opacity="30" XOffset="2" YOffset="2" Spread="10"/></TransparencySetting>
    </ObjectStyle>"#;

#[test]
fn frames_take_corners_and_drop_shadow_from_their_object_style() {
    let d = fixture(
        ROUND,
        &[
            (r#"AppliedObjectStyle="ObjectStyle/Round""#, ""),
            (
                r#"AppliedObjectStyle="ObjectStyle/Round" TopLeftCornerRadius="13.98" TopRightCornerRadius="13.98"
                   BottomLeftCornerRadius="13.98" BottomRightCornerRadius="13.98" CornerRadius="13.98""#,
                r#"<TransparencySetting><DropShadowSetting XOffset="2.33" YOffset="2.33" Size="5.825"/></TransparencySetting>"#,
            ),
        ],
    );
    let items = &d.spreads[0].items;
    // Style only.
    assert_round(&items[0], 17.0);
    let ds = &items[0].effects.drop_shadow;
    assert!(ds.on);
    assert!((ds.distance - 2.0f64.hypot(2.0)).abs() < 1e-9);
    assert!((ds.opacity - 0.3).abs() < 1e-6);
    assert_eq!((ds.size, ds.spread), (5.0, 10.0), "size from [None], spread from the style");
    // Local radius and offsets over the style's corner shape and shadow.
    assert_round(&items[1], 13.98);
    let ds = &items[1].effects.drop_shadow;
    assert!(ds.on);
    assert!((ds.distance - 2.33f64.hypot(2.33)).abs() < 1e-9);
    assert!((ds.opacity - 0.3).abs() < 1e-6);
    assert_eq!((ds.size, ds.spread), (5.825, 10.0));
}

#[test]
fn values_resolve_through_the_based_on_chain_attribute_by_attribute() {
    let styles = format!(
        r#"{ROUND}
        <ObjectStyle Self="ObjectStyle/Wrapped" Name="Wrapped">
          <Properties><BasedOn type="object">ObjectStyle/Round</BasedOn></Properties>
          <TransparencySetting><BlendingSetting Opacity="80"/><DropShadowSetting Opacity="50"/></TransparencySetting>
          <TextWrapPreference TextWrapMode="BoundingBoxTextWrap"><Properties><TextWrapOffset Top="4" Left="4" Bottom="4" Right="4"/></Properties></TextWrapPreference>
        </ObjectStyle>"#
    );
    let d = fixture(
        &styles,
        &[(
            r#"AppliedObjectStyle="ObjectStyle/Wrapped" TopRightCornerOption="BevelCorner""#,
            r#"<TextWrapPreference><Properties><TextWrapOffset Top="9"/></Properties></TextWrapPreference>"#,
        )],
    );
    let it = &d.spreads[0].items[0];
    let shapes: Vec<CornerShape> = it.corners.corners.iter().map(|c| c.shape).collect();
    assert_eq!(shapes.iter().filter(|s| **s == CornerShape::Rounded).count(), 3, "{shapes:?}");
    assert_eq!(shapes.iter().filter(|s| **s == CornerShape::Bevel).count(), 1, "{shapes:?}");
    assert!(it.corners.corners.iter().all(|c| c.size == 17.0));
    assert!((it.opacity - 0.8).abs() < 1e-6);
    let ds = &it.effects.drop_shadow;
    assert!(ds.on, "Mode from the grandparent style");
    assert!((ds.opacity - 0.5).abs() < 1e-6);
    assert!((ds.distance - 2.0f64.hypot(2.0)).abs() < 1e-9);
    assert_eq!(it.wrap.mode, designcraft_doc::WrapMode::BoundingBox);
    assert_eq!(it.wrap.offsets, [9.0, 4.0, 4.0, 4.0]);
}

#[test]
fn item_overrides_and_disabled_categories_turn_style_effects_off() {
    let styles = format!(
        r#"{ROUND}
        <ObjectStyle Self="ObjectStyle/NoShadow" Name="NoShadow">
          <Properties><BasedOn type="object">ObjectStyle/Round</BasedOn></Properties>
          <ObjectStyleObjectEffectsCategorySettings EnableDropShadow="false"/>
        </ObjectStyle>"#
    );
    let d = fixture(
        &styles,
        &[
            (
                r#"AppliedObjectStyle="ObjectStyle/Round" TopLeftCornerOption="None" TopRightCornerOption="None" BottomLeftCornerOption="None" BottomRightCornerOption="None""#,
                r#"<TransparencySetting><DropShadowSetting Mode="None"/></TransparencySetting>"#,
            ),
            (r#"AppliedObjectStyle="ObjectStyle/NoShadow""#, ""),
            (r#"AppliedObjectStyle="ObjectStyle/NoShadow""#, r#"<TransparencySetting><DropShadowSetting Mode="Drop"/></TransparencySetting>"#),
        ],
    );
    let items = &d.spreads[0].items;
    assert!(items[0].corners.is_none());
    assert!(!items[0].effects.drop_shadow.on);
    assert!(!items[1].effects.drop_shadow.on);
    assert_round(&items[1], 17.0);
    // The item's own shadow, without the disabled style's values.
    let ds = &items[2].effects.drop_shadow;
    assert!(ds.on);
    assert!((ds.opacity - 0.75).abs() < 1e-6);
}

/// An object style's paragraph style and Apply Next Style (`ApplyNextParagraphStyle`) round-trip.
#[test]
fn object_style_apply_next_style_round_trips() {
    let mut d = small_doc();
    let styles = std::sync::Arc::make_mut(&mut d.styles);
    styles.paragraph.push(designcraft_doc::ParagraphStyle {
        name: "Opener".into(),
        based_on: None,
        next_style: Some(designcraft_doc::BASIC_PARAGRAPH.into()),
        para: Default::default(),
        chars: Default::default(),
        shortcut: String::new(),
    });
    for (name, next) in [("Chained", true), ("Single", false)] {
        styles.object.push(designcraft_doc::ObjectStyle {
            name: name.into(),
            paragraph_style: Some("Opener".into()),
            apply_next_style: next,
            ..Default::default()
        });
    }
    let back = import_idml(&export_idml(&d)).unwrap();
    for (name, next) in [("Chained", true), ("Single", false)] {
        let os = back.styles.object_style(name).unwrap();
        assert_eq!((os.paragraph_style.as_deref(), os.apply_next_style), (Some("Opener"), next), "{name}");
    }
}

/// A style that sets no corners keeps none after export and import, though the style it is based
/// on sets rounded corners.
#[test]
fn a_style_without_corners_keeps_none_over_its_based_on_style() {
    let mut d = small_doc();
    let rounded = Some(designcraft_geom::corners::CornerOptions::uniform(CornerShape::Rounded, 12.0));
    let styles = std::sync::Arc::make_mut(&mut d.styles);
    styles.object.push(designcraft_doc::ObjectStyle { name: "Round".into(), corners: rounded, ..Default::default() });
    styles.object.push(designcraft_doc::ObjectStyle { name: "Square".into(), based_on: Some("Round".into()), ..Default::default() });
    let back = import_idml(&export_idml(&d)).unwrap();
    assert_eq!(back.styles.object_style("Round").and_then(|s| s.corners), rounded);
    assert_eq!(back.styles.object_style("Square").and_then(|s| s.corners), None);
}

/// A style's Text Frame General Options take each setting its own `TextFramePreference` leaves out
/// from the style it is based on (named by its id or bare name, and only when that style doesn't
/// turn the category off), and export and import keep the merged options.
#[test]
fn object_style_text_frame_options_inherit_from_the_based_on_style() {
    let styles = r#"<ObjectStyle Self="ObjectStyle/Box" Name="Box" EnableTextFrameGeneralOptions="true">
      <Properties><BasedOn type="string">$ID/[None]</BasedOn></Properties>
      <TextFramePreference TextColumnCount="2" VerticalJustification="CenterAlign"/>
    </ObjectStyle>
    <ObjectStyle Self="ObjectStyle/Inset" Name="Inset" EnableTextFrameGeneralOptions="true">
      <Properties><BasedOn type="object">ObjectStyle/Box</BasedOn></Properties>
      <TextFramePreference InsetSpacing="9"/>
    </ObjectStyle>
    <ObjectStyle Self="ObjectStyle/Bare" Name="Bare" EnableTextFrameGeneralOptions="true">
      <Properties><BasedOn type="string">Box</BasedOn></Properties>
      <TextFramePreference InsetSpacing="3"/>
    </ObjectStyle>
    <ObjectStyle Self="ObjectStyle/Off" Name="Off" EnableTextFrameGeneralOptions="false">
      <Properties><BasedOn type="string">$ID/[None]</BasedOn></Properties>
      <TextFramePreference TextColumnCount="5"/>
    </ObjectStyle>
    <ObjectStyle Self="ObjectStyle/OnOff" Name="OnOff" EnableTextFrameGeneralOptions="true">
      <Properties><BasedOn type="object">ObjectStyle/Off</BasedOn></Properties>
      <TextFramePreference InsetSpacing="1"/>
    </ObjectStyle>"#;
    let d = fixture(styles, &[]);
    let tf = d.styles.object_style("Inset").and_then(|s| s.text_frame.clone()).expect("Inset sets text frame options");
    assert_eq!((tf.inset, tf.columns, tf.vertical_justification), ([9.0; 4], 2, designcraft_doc::VerticalJustification::Center));
    let bare = d.styles.object_style("Bare").and_then(|s| s.text_frame.clone()).expect("Bare sets text frame options");
    assert_eq!((bare.inset, bare.columns), ([3.0; 4], 2), "a BasedOn given as a bare name");
    let on_off = d.styles.object_style("OnOff").and_then(|s| s.text_frame.clone()).expect("OnOff sets text frame options");
    assert_eq!((on_off.inset, on_off.columns), ([1.0; 4], 1), "a based-on style with the category off gives nothing");
    let back = import_idml(&export_idml(&d)).unwrap();
    let again = back.styles.object_style("Inset").and_then(|s| s.text_frame.clone()).expect("after a round trip");
    assert!(again.general_eq(&tf), "{again:?}");
}

#[test]
fn exported_frames_keep_their_own_values_over_their_object_style() {
    let mut d = small_doc();
    let style = designcraft_doc::TextFrameOptions { columns: 2, column_rule: true, column_rule_weight: 3.0, ..Default::default() };
    std::sync::Arc::make_mut(&mut d.styles).object.push(designcraft_doc::ObjectStyle {
        name: "Ruled".into(),
        text_frame: Some(style),
        ..Default::default()
    });
    let fid = d.spreads[0].items[0].id;
    let it = d.item_mut(fid).unwrap();
    it.object_style = "Ruled".into();
    let want = it.text_frame().unwrap().options.clone();
    let back = import_idml(&export_idml(&d)).unwrap();
    let got = back.spreads[0].items.iter().find(|i| i.object_style == "Ruled").and_then(|i| i.text_frame()).unwrap();
    assert_eq!(got.options, want);
}

/// Items without corners keep none after export and import, though their object style, the style
/// it is based on, or `[None]` (read after every style chain) sets rounded corners.
#[test]
fn exported_frames_keep_no_corners_over_their_object_style() {
    let mut d = small_doc();
    let lid = d.default_layer();
    let id = designcraft_doc::ItemId(d.alloc());
    let rect = designcraft_doc::Item::new(id, lid, Shape::Rectangle, shapes::rectangle(Rect::new(40.0, 420.0, 200.0, 500.0)));
    d.insert_item(SpreadRef::Doc(0), rect, None).unwrap();
    let rounded = Some(designcraft_geom::corners::CornerOptions {
        corners: [designcraft_geom::corners::Corner { shape: CornerShape::Rounded, size: 12.0 }; 4],
    });
    let styles = std::sync::Arc::make_mut(&mut d.styles);
    styles.object.push(designcraft_doc::ObjectStyle { name: "Round".into(), corners: rounded, ..Default::default() });
    styles.object.push(designcraft_doc::ObjectStyle { name: "Child".into(), based_on: Some("Round".into()), ..Default::default() });
    match styles.object.iter_mut().find(|o| o.name == designcraft_doc::NO_OBJECT_STYLE) {
        Some(none) => none.corners = rounded,
        None => {
            styles.object.push(designcraft_doc::ObjectStyle { name: designcraft_doc::NO_OBJECT_STYLE.into(), corners: rounded, ..Default::default() })
        }
    }
    styles.object.push(designcraft_doc::ObjectStyle { name: "Plain".into(), ..Default::default() });
    let names = ["Round", "Child", designcraft_doc::NO_OBJECT_STYLE, "Plain"];
    let ids: Vec<_> = d.spreads.iter().flat_map(|s| s.items.iter().map(|i| i.id)).take(4).collect();
    assert_eq!(ids.len(), 4);
    for (id, style) in ids.iter().zip(names) {
        let it = d.item_mut(*id).unwrap();
        it.object_style = style.into();
        assert!(it.corners.is_none());
    }
    let back = import_idml(&export_idml(&d)).unwrap();
    for style in names {
        let it = back.spreads.iter().flat_map(|s| s.items.iter()).find(|i| i.object_style == style).unwrap();
        assert!(it.corners.is_none(), "{style}: {:?}", it.corners);
    }
}

#[test]
fn text_wrap_side_and_contour_come_from_the_style_for_frames_and_placed_graphics() {
    let styles = r#"<ObjectStyle Self="ObjectStyle/Contour" Name="Contour">
          <Properties><BasedOn type="string">$ID/[None]</BasedOn></Properties>
          <TextWrapPreference TextWrapMode="Contour" TextWrapSide="LeftSide">
            <Properties><TextWrapOffset Top="3" Left="3" Bottom="3" Right="3"/></Properties>
            <ContourOption ContourType="BoundingBox"/>
          </TextWrapPreference>
        </ObjectStyle>"#;
    let image = r#"<Image Self="g" AppliedObjectStyle="ObjectStyle/Contour">
          <TextWrapPreference TextWrapSide="RightSide"/>
        </Image>"#;
    let d = fixture(styles, &[(r#"AppliedObjectStyle="ObjectStyle/Contour""#, ""), ("", image)]);
    let frame = &d.spreads[0].items[0].wrap;
    assert_eq!(frame.mode, designcraft_doc::WrapMode::Contour);
    assert_eq!(frame.side, designcraft_doc::WrapSide::LeftSide);
    assert_eq!(frame.contour, designcraft_doc::ContourType::BoundingBox);
    assert_eq!(frame.offsets, [3.0; 4]);
    let designcraft_doc::Content::Graphic(g) = &d.spreads[0].items[1].content else { panic!("{:?}", d.spreads[0].items[1].content) };
    assert_eq!(g.wrap.mode, designcraft_doc::WrapMode::Contour);
    assert_eq!(g.wrap.side, designcraft_doc::WrapSide::RightSide, "the graphic's own side wins");
    assert_eq!(g.wrap.contour, designcraft_doc::ContourType::BoundingBox);
}

#[test]
fn feathers_come_from_the_object_style_unless_its_category_turns_them_off() {
    let styles = r#"<ObjectStyle Self="ObjectStyle/Fade" Name="Fade">
          <Properties><BasedOn type="string">$ID/[None]</BasedOn></Properties>
          <TransparencySetting>
            <GradientFeatherSetting Applied="true" Type="Radial">
              <OpacityGradientStop Self="s0" Opacity="100" Location="0"/>
              <OpacityGradientStop Self="s1" Opacity="0" Location="100"/>
            </GradientFeatherSetting>
            <DirectionalFeatherSetting Applied="true" LeftWidth="3" RightWidth="4" TopWidth="5" BottomWidth="6"/>
          </TransparencySetting>
        </ObjectStyle>
        <ObjectStyle Self="ObjectStyle/NoGradient" Name="NoGradient">
          <Properties><BasedOn type="object">ObjectStyle/Fade</BasedOn></Properties>
          <ObjectStyleObjectEffectsCategorySettings EnableGradientFeather="false"/>
        </ObjectStyle>"#;
    let d = fixture(
        styles,
        &[
            (r#"AppliedObjectStyle="ObjectStyle/Fade""#, ""),
            (r#"AppliedObjectStyle="ObjectStyle/NoGradient""#, ""),
            (
                r#"AppliedObjectStyle="ObjectStyle/Fade""#,
                r#"<TransparencySetting><DirectionalFeatherSetting Applied="false"/></TransparencySetting>"#,
            ),
        ],
    );
    let fx: Vec<&designcraft_doc::Effects> = d.spreads[0].items.iter().map(|i| &i.effects).collect();
    assert!(fx[0].gradient_feather.on && fx[0].gradient_feather.radial, "{:?}", fx[0]);
    assert!(fx[0].directional_feather.on);
    assert_eq!(fx[0].directional_feather.widths, [5.0, 3.0, 6.0, 4.0]);
    assert!(!fx[1].gradient_feather.on, "the category flag turns the style's gradient feather off");
    assert!(fx[1].directional_feather.on);
    assert!(fx[2].gradient_feather.on);
    assert!(!fx[2].directional_feather.on, "the item's own setting wins");
}

/// A style that sets one corner takes the others from the style it is based on, so applying it
/// keeps the based-on style's rounded corners; and a style's named corners go to the anchors at
/// those corners of a frame whose path runs top left, bottom left, bottom right, top right.
#[test]
fn a_style_setting_one_corner_inherits_the_others_and_names_them() {
    let child = r#"<ObjectStyle Self="ObjectStyle/Child" Name="Child" TopRightCornerOption="BevelCorner" TopRightCornerRadius="4">
      <Properties><BasedOn type="object">ObjectStyle/Round</BasedOn></Properties>
    </ObjectStyle>"#;
    let d = fixture(&format!("{ROUND}{child}"), &[(r#"AppliedObjectStyle="ObjectStyle/Child""#, "")]);
    let style = d.styles.object.iter().find(|s| s.name == "Child").unwrap();
    let c = style.corners.expect("the child sets corners").corners;
    assert_eq!(c.map(|c| c.shape), [CornerShape::Rounded, CornerShape::Bevel, CornerShape::Rounded, CornerShape::Rounded], "{c:?}");
    assert_eq!(c.map(|c| c.size), [17.0, 4.0, 17.0, 17.0]);
    // Applied to the fixture's frame (anchors top left, bottom left, bottom right, top right):
    // the bevel goes to the top right anchor, the last.
    let mut it = (*d.spreads[0].items[0]).clone();
    it.corners = Default::default();
    style.apply_to(&mut it);
    assert_eq!(it.corners.corners.map(|c| c.shape), [CornerShape::Rounded, CornerShape::Rounded, CornerShape::Rounded, CornerShape::Bevel]);
}

/// A style that turns Stroke & Corner Options on without corner attributes of its own takes all
/// four corners from the style it is based on.
#[test]
fn a_style_turning_corners_on_inherits_all_four() {
    let child = r#"<ObjectStyle Self="ObjectStyle/Plain" Name="Plain" EnableStrokeAndCornerOptions="true">
      <Properties><BasedOn type="object">ObjectStyle/Round</BasedOn></Properties>
    </ObjectStyle>"#;
    let d = fixture(&format!("{ROUND}{child}"), &[]);
    let c = d.styles.object.iter().find(|s| s.name == "Plain").and_then(|s| s.corners).expect("inherited corners").corners;
    assert_eq!((c.map(|c| c.shape), c.map(|c| c.size)), ([CornerShape::Rounded; 4], [17.0; 4]));
}

/// A style with no Stroke & Corner Options or Text Frame General Options flag and no settings of
/// its own takes each category from the style it is based on: on, off, or (for a style based on
/// `[None]`) not set.
#[test]
fn a_style_without_category_flags_takes_them_from_its_based_on_style() {
    let styles = r#"<ObjectStyle Self="ObjectStyle/Framed" Name="Framed" EnableTextFrameGeneralOptions="true">
      <Properties><BasedOn type="string">$ID/[None]</BasedOn></Properties>
      <TextFramePreference TextColumnCount="3"/>
    </ObjectStyle>
    <ObjectStyle Self="ObjectStyle/Off" Name="Off" EnableStrokeAndCornerOptions="false" EnableTextFrameGeneralOptions="false"
      TopLeftCornerOption="RoundedCorner">
      <Properties><BasedOn type="object">ObjectStyle/Round</BasedOn></Properties>
      <TextFramePreference TextColumnCount="4"/>
    </ObjectStyle>
    <ObjectStyle Self="ObjectStyle/RoundKid" Name="RoundKid">
      <Properties><BasedOn type="object">ObjectStyle/Round</BasedOn></Properties>
    </ObjectStyle>
    <ObjectStyle Self="ObjectStyle/FramedKid" Name="FramedKid">
      <Properties><BasedOn type="object">ObjectStyle/Framed</BasedOn></Properties>
    </ObjectStyle>
    <ObjectStyle Self="ObjectStyle/OffKid" Name="OffKid">
      <Properties><BasedOn type="object">ObjectStyle/Off</BasedOn></Properties>
    </ObjectStyle>
    <ObjectStyle Self="ObjectStyle/Bare" Name="Bare">
      <Properties><BasedOn type="string">$ID/[None]</BasedOn></Properties>
    </ObjectStyle>"#;
    let d = fixture(&format!("{ROUND}{styles}"), &[]);
    let style = |n: &str| d.styles.object_style(n).cloned().unwrap();
    let c = style("RoundKid").corners.expect("RoundKid inherits Round's corners").corners;
    assert_eq!((c.map(|c| c.shape), c.map(|c| c.size)), ([CornerShape::Rounded; 4], [17.0; 4]));
    assert_eq!(style("FramedKid").text_frame.map(|t| t.columns), Some(3), "FramedKid inherits Framed's text frame options");
    for n in ["OffKid", "Bare"] {
        assert!(style(n).corners.is_none() && style(n).text_frame.is_none(), "{n}: {:?}", style(n));
    }
    assert!(style(designcraft_doc::NO_OBJECT_STYLE).corners.is_some(), "[None] keeps the corners it sets itself");
}

/// A style without a Fill, Stroke or Paragraph Styles flag or value of its own takes each category
/// from the style it is based on, and a style turning a category on takes the values it leaves out
/// from there; a based-on style that turns a category off gives nothing.
#[test]
fn fill_stroke_and_paragraph_style_categories_come_through_the_based_on_chain() {
    let styles = r#"<ObjectStyle Self="ObjectStyle/Brand" Name="Brand" EnableFill="true" FillColor="Color/Black" FillTint="40"
      EnableStroke="true" StrokeColor="Color/Black" StrokeWeight="3" EnableParagraphStyle="true"
      AppliedParagraphStyle="ParagraphStyle/Opener" ApplyNextParagraphStyle="true">
      <Properties><BasedOn type="string">$ID/[None]</BasedOn></Properties>
    </ObjectStyle>
    <ObjectStyle Self="ObjectStyle/Kid" Name="Kid">
      <Properties><BasedOn type="object">ObjectStyle/Brand</BasedOn></Properties>
    </ObjectStyle>
    <ObjectStyle Self="ObjectStyle/Tint" Name="Tint" EnableFill="true" FillTint="70" EnableStroke="true" StrokeWeight="5">
      <Properties><BasedOn type="object">ObjectStyle/Brand</BasedOn></Properties>
    </ObjectStyle>
    <ObjectStyle Self="ObjectStyle/Off" Name="Off" EnableFill="false" EnableStroke="false" EnableParagraphStyle="false">
      <Properties><BasedOn type="object">ObjectStyle/Brand</BasedOn></Properties>
    </ObjectStyle>
    <ObjectStyle Self="ObjectStyle/OffKid" Name="OffKid">
      <Properties><BasedOn type="object">ObjectStyle/Off</BasedOn></Properties>
    </ObjectStyle>"#;
    let d = fixture(styles, &[]);
    let style = |n: &str| d.styles.object_style(n).cloned().unwrap();
    let brand = style("Brand");
    let (brand_fill, brand_stroke) = (brand.fill.clone().expect("Brand sets a fill"), brand.stroke.clone().expect("Brand sets a stroke"));
    let kid = style("Kid");
    assert_eq!((kid.fill.as_ref(), kid.stroke.as_ref()), (Some(&brand_fill), Some(&brand_stroke)), "Kid inherits Brand's fill and stroke");
    assert_eq!((kid.paragraph_style.as_deref(), kid.apply_next_style), (Some("Opener"), true), "Kid inherits Brand's paragraph style");
    let t = style("Tint");
    let (fill, stroke) = (t.fill.expect("Tint sets a fill"), t.stroke.expect("Tint sets a stroke"));
    assert_eq!(fill.swatch, brand_fill.swatch, "Tint takes Brand's fill color");
    assert!((fill.tint - 0.7).abs() < 1e-6 && (brand_fill.tint - 0.4).abs() < 1e-6, "{fill:?} {brand_fill:?}");
    assert_eq!((stroke.swatch, stroke.weight), (brand_stroke.swatch.clone(), 5.0), "Tint takes Brand's stroke color");
    for n in ["Off", "OffKid"] {
        let s = style(n);
        assert!(s.fill.is_none() && s.stroke.is_none() && s.paragraph_style.is_none(), "{n}: {s:?}");
    }
}

/// A style that turns a category on takes none of its values from beyond a based-on style that
/// turns the category off.
#[test]
fn category_values_stop_at_a_based_on_style_that_turns_the_category_off() {
    let styles = r#"<ObjectStyle Self="ObjectStyle/Full" Name="Full" EnableFill="true" FillColor="Color/Black"
      EnableTextFrameGeneralOptions="true" EnableStrokeAndCornerOptions="true" TopLeftCornerOption="RoundedCorner" TopLeftCornerRadius="17">
      <Properties><BasedOn type="string">$ID/[None]</BasedOn></Properties>
      <TextFramePreference TextColumnCount="3"/>
    </ObjectStyle>
    <ObjectStyle Self="ObjectStyle/Off" Name="Off" EnableFill="false" EnableTextFrameGeneralOptions="false" EnableStrokeAndCornerOptions="false">
      <Properties><BasedOn type="object">ObjectStyle/Full</BasedOn></Properties>
    </ObjectStyle>
    <ObjectStyle Self="ObjectStyle/Gap" Name="Gap" EnableFill="true" EnableTextFrameGeneralOptions="true" EnableStrokeAndCornerOptions="true">
      <Properties><BasedOn type="object">ObjectStyle/Off</BasedOn></Properties>
    </ObjectStyle>"#;
    let d = fixture(styles, &[]);
    let gap = d.styles.object_style("Gap").cloned().unwrap();
    assert_eq!(gap.fill.as_ref().map(|f| f.swatch.as_str()), Some(designcraft_color::swatch::NONE), "{:?}", gap.fill);
    assert_ne!(gap.text_frame.as_ref().map(|t| t.columns), Some(3), "{:?}", gap.text_frame);
    assert!(gap.corners.is_none(), "{:?}", gap.corners);
}

/// A style based on `[None]` that turns a category on takes none of that category's values from
/// `[None]`, which keeps its own.
#[test]
fn category_values_stop_at_none() {
    let designmap = r#"<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" Self="d">
      <RootObjectStyleGroup Self="ro">
        <ObjectStyle Self="ObjectStyle/$ID/[None]" Name="$ID/[None]" EnableFill="true" FillColor="Color/Black" EnableStroke="true"
          StrokeColor="Color/Black" StrokeWeight="0"/>
        <ObjectStyle Self="ObjectStyle/Plain" Name="Plain" EnableFill="true" FillTint="70" EnableStroke="true" StrokeColor="Color/Black">
          <Properties><BasedOn type="string">$ID/[None]</BasedOn></Properties>
        </ObjectStyle>
      </RootObjectStyleGroup>
      <idPkg:Spread src="Spreads/Spread_s.xml"/>
    </Document>"#;
    let spread = r#"<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
      <Spread Self="s"><Page Self="p" GeometricBounds="0 0 200 200" ItemTransform="1 0 0 1 0 0"/></Spread>
    </idPkg:Spread>"#;
    let d = import_idml(&zip_files(&[("designmap.xml", designmap), ("Spreads/Spread_s.xml", spread)])).unwrap();
    let plain = d.styles.object_style("Plain").cloned().unwrap();
    assert_eq!(plain.fill.as_ref().map(|f| f.swatch.as_str()), Some(designcraft_color::swatch::NONE), "{:?}", plain.fill);
    assert_eq!(plain.stroke.as_ref().map(|s| s.weight), Some(designcraft_doc::Stroke::none().weight), "{:?}", plain.stroke);
    let none = d.styles.object_style(designcraft_doc::NO_OBJECT_STYLE).cloned().unwrap();
    assert_eq!(none.stroke.map(|s| s.weight), Some(0.0), "[None] keeps its own stroke weight");
}

/// An object style giving its corners only with the legacy all-corners `CornerOption` and
/// `CornerRadius` keeps them, so applying it to another frame rounds it.
#[test]
fn a_style_with_legacy_all_corner_attributes_keeps_its_corners() {
    let legacy = r#"<ObjectStyle Self="ObjectStyle/Legacy" Name="Legacy" CornerOption="RoundedCorner" CornerRadius="9">
      <Properties><BasedOn type="string">$ID/[None]</BasedOn></Properties>
    </ObjectStyle>"#;
    let d = fixture(legacy, &[]);
    let style = d.styles.object.iter().find(|s| s.name == "Legacy").unwrap();
    let c = style.corners.expect("the legacy corners").corners;
    assert!(c.iter().all(|c| c.shape == CornerShape::Rounded && c.size == 9.0), "{c:?}");
}
