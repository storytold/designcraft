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
