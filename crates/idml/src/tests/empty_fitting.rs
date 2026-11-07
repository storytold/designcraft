use super::*;

const NS: &str = r#"xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0""#;

fn frame(id: &str, x0: f64, fitting: &str) -> String {
    let (x1, y0, y1) = (x0 + 80.0, 100.0, 160.0);
    let pts: String = [(x0, y0), (x0, y1), (x1, y1), (x1, y0)]
        .iter()
        .map(|(x, y)| format!(r#"<PathPointType Anchor="{x} {y}" LeftDirection="{x} {y}" RightDirection="{x} {y}"/>"#))
        .collect();
    format!(
        r#"<Rectangle Self="{id}" ItemLayer="L1" ContentType="GraphicType" ItemTransform="1 0 0 1 0 -300"><Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>{pts}</PathPointArray></GeometryPathType></PathGeometry></Properties>{fitting}</Rectangle>"#
    )
}

/// Three empty graphic frames: one fitting proportionally from the top-left with Auto-Fit and a
/// crop, one whose options set nothing, and one without options.
fn package() -> Vec<u8> {
    package_with(STYLES, "")
}

/// The package of [`package`] with `styles` as its styles part and `extra` frames on the page.
fn package_with(styles: &str, extra: &str) -> Vec<u8> {
    let designmap = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Document {NS} Self="d"><idPkg:Graphic src="Resources/Graphic.xml"/><idPkg:Styles src="Resources/Styles.xml"/><idPkg:Preferences src="Resources/Preferences.xml"/><Layer Self="L1" Name="Layer 1" Visible="true" Locked="false"/><idPkg:Spread src="Spreads/Spread_sp1.xml"/></Document>"#
    );
    let prefs = format!(r#"<idPkg:Preferences {NS}><DocumentPreference PageWidth="400" PageHeight="600" FacingPages="false"/></idPkg:Preferences>"#);
    let spread = format!(
        r#"<idPkg:Spread {NS}><Spread Self="sp1" PageCount="1" ItemTransform="1 0 0 1 0 0"><Page Self="p1" GeometricBounds="0 0 600 400" ItemTransform="1 0 0 1 0 -300"/>{}{}{}</Spread></idPkg:Spread>"#,
        frame(
            "r1",
            20.0,
            r#"<FrameFittingOption AutoFit="true" LeftCrop="1" TopCrop="2" RightCrop="3" BottomCrop="4" FittingOnEmptyFrame="Proportionally" FittingAlignment="TopLeftAnchor"/>"#
        ),
        frame(
            "r2",
            120.0,
            r#"<FrameFittingOption AutoFit="false" LeftCrop="0" TopCrop="0" RightCrop="0" BottomCrop="0" FittingOnEmptyFrame="None" FittingAlignment="CenterAnchor"/>"#
        ),
        frame("r3", 220.0, ""),
    )
    .replacen("</Spread>", &format!("{extra}</Spread>"), 1);
    zip_files(&[
        ("designmap.xml", &designmap),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Styles.xml", styles),
        ("Resources/Preferences.xml", &prefs),
        ("Spreads/Spread_sp1.xml", &spread),
    ])
}

fn fittings(d: &Document) -> Vec<Option<designcraft_doc::EmptyFrameFitting>> {
    d.spreads[0].items.iter().map(|it| it.empty_fitting).collect()
}

fn expected() -> Vec<Option<designcraft_doc::EmptyFrameFitting>> {
    let fit = designcraft_doc::EmptyFrameFitting {
        fitting: designcraft_doc::Fitting::FitProportionally,
        auto_fit: true,
        align: 0,
        crop: [2.0, 1.0, 4.0, 3.0],
    };
    vec![Some(fit), None, None]
}

#[test]
fn an_empty_frames_fitting_options_are_read() {
    let d = import_idml_with(&package(), &|_| None).unwrap();
    assert!(d.spreads[0].items.iter().all(|it| matches!(it.content, designcraft_doc::Content::Unassigned)));
    assert_eq!(fittings(&d), expected());
}

#[test]
fn an_empty_frames_fitting_options_survive_a_round_trip() {
    let d = import_idml_with(&package(), &|_| None).unwrap();
    let bytes = export_idml(&d);
    let spreads = zip_text(&bytes, "Spreads/");
    assert_eq!(spreads.matches("<FrameFittingOption").count(), 1, "options that set nothing are not written: {spreads}");
    assert!(spreads.contains(r#"FittingOnEmptyFrame="Proportionally" FittingAlignment="TopLeftAnchor""#), "{spreads}");
    assert_eq!(fittings(&import_idml(&bytes).unwrap()), expected());
}

#[test]
fn an_empty_frame_takes_fitting_options_from_its_object_style_unless_it_sets_its_own() {
    let styles = STYLES.replace(
        "</RootObjectStyleGroup>",
        r#"<ObjectStyle Self="ObjectStyle/Logo" Name="Logo"><FrameFittingOption AutoFit="false" FittingOnEmptyFrame="FillProportionally" FittingAlignment="TopLeftAnchor"/></ObjectStyle></RootObjectStyleGroup>"#,
    );
    let styled = |id: &str, x0: f64, own: &str| frame(id, x0, own).replacen("<Rectangle ", r#"<Rectangle AppliedObjectStyle="ObjectStyle/Logo" "#, 1);
    let extra = styled("r4", 320.0, "") + &styled("r5", 420.0, r#"<FrameFittingOption FittingOnEmptyFrame="Proportionally"/>"#);
    let d = import_idml_with(&package_with(&styles, &extra), &|_| None).unwrap();
    let f = |i: usize| d.spreads[0].items[i].empty_fitting;
    let inherited = f(3).expect("the style's options");
    assert_eq!((inherited.fitting, inherited.align), (designcraft_doc::Fitting::FillProportionally, 0));
    let own = f(4).expect("the frame's own options over the style's");
    assert_eq!((own.fitting, own.align), (designcraft_doc::Fitting::FitProportionally, 0), "unset attributes still come from the style");
    let back = import_idml(&export_idml(&d)).unwrap();
    assert_eq!((back.spreads[0].items[3].empty_fitting, back.spreads[0].items[4].empty_fitting), (f(3), f(4)));
}
