//! Literal guide types must survive independently of the stored page layout rule (#872).

use super::*;
use designcraft_doc::{LiquidRule, Orientation};
use std::sync::Arc;

fn fixture(kind: Option<&str>, origin: &str) -> Vec<u8> {
    let attribute = kind.map(|k| format!(r#" GuideType="{k}""#)).unwrap_or_default();
    let map = r#"<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" Self="d">
  <idPkg:Preferences src="Resources/Preferences.xml"/>
  <Layer Self="L1" Name="Guides" Visible="true" Locked="false"/>
  <idPkg:MasterSpread src="MasterSpreads/A.xml"/>
  <idPkg:Spread src="Spreads/S.xml"/>
</Document>"#;
    let prefs = format!(
        r#"<Preferences><DocumentPreference PageWidth="500" PageHeight="700" FacingPages="true"/>
<ViewPreference RulerOrigin="{origin}"/></Preferences>"#
    );
    let spread = |tag: &str, id: &str| {
        let pages: String = [("left", -500), ("right", 0)]
            .into_iter()
            .map(|(side, x)| {
                format!(
                    r#"<Page Self="{id}-{side}" LayoutRule="GuideBased" GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 {x} -350">
  <Guide Self="{id}-{side}-v" Orientation="Vertical" Location="72" FitToPage="true" Locked="true" ItemLayer="L1"{attribute}/>
  <Guide Self="{id}-{side}-h" Orientation="Horizontal" Location="90" FitToPage="false" Locked="false" ItemLayer="L1" GuideType="Ruler"/>
</Page>"#
                )
            })
            .collect();
        format!(
            r#"<idPkg:{tag} xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
<{tag} Self="{id}" NamePrefix="A" BaseName="Parent" PageCount="2">{pages}</{tag}>
</idPkg:{tag}>"#
        )
    };
    zip_files(&[
        ("designmap.xml", map),
        ("Resources/Preferences.xml", &prefs),
        ("MasterSpreads/A.xml", &spread("MasterSpread", "m")),
        ("Spreads/S.xml", &spread("Spread", "s")),
    ])
}

#[test]
fn liquid_guides_import_literal_types_without_changing_other_guide_fields() {
    for origin in ["SpreadOrigin", "PageOrigin", "SpineOrigin"] {
        let baseline = import_idml(&fixture(None, origin)).unwrap();
        for (kind, liquid) in [(Some("Liquid"), true), (Some("Ruler"), false), (None, false), (Some("liquid"), false), (Some("Unknown"), false)] {
            let actual = import_idml(&fixture(kind, origin)).unwrap();
            let mut expected = baseline.clone();
            expected.created = actual.created;
            for spread in expected.spreads.iter_mut().chain(&mut expected.parents) {
                for page in &mut Arc::make_mut(spread).pages {
                    assert_eq!(page.guides.len(), 2);
                    page.guides[0].liquid = liquid;
                }
            }
            assert_eq!(actual, expected, "{origin}, {kind:?}: only guide type may change");
            actual.check().unwrap();
            for spread in actual.spreads.iter().chain(&actual.parents) {
                assert_eq!(spread.pages.len(), 2);
                for (index, page) in spread.pages.iter().enumerate() {
                    assert_eq!(page.liquid, LiquidRule::GuideBased);
                    let [vertical, horizontal] = page.guides.as_slice() else { panic!("two guides expected") };
                    let x = match origin {
                        "PageOrigin" => 72.0 + index as f64 * 500.0,
                        "SpineOrigin" => 572.0,
                        _ => 72.0,
                    };
                    assert_eq!((vertical.orientation, vertical.position, vertical.spread, vertical.locked), (Orientation::Vertical, x, false, true));
                    assert_eq!(
                        (horizontal.orientation, horizontal.position, horizontal.spread, horizontal.locked),
                        (Orientation::Horizontal, 90.0, true, false)
                    );
                    assert!(!horizontal.liquid);
                    assert_eq!(actual.layer(vertical.layer.unwrap()).unwrap().name, "Guides");
                }
            }
        }
    }
}

#[test]
fn liquid_guides_export_literal_types_and_round_trip_on_pages_and_parents() {
    use std::io::Read;

    let mut source = import_idml(&fixture(None, "PageOrigin")).unwrap();
    // Build the mixed native flags directly, without relying on the liquid-guide importer.
    for spread in source.spreads.iter_mut().chain(&mut source.parents) {
        for (index, page) in Arc::make_mut(spread).pages.iter_mut().enumerate() {
            page.guides[index].liquid = true;
            if index == 1 {
                page.liquid = LiquidRule::Off;
            }
        }
    }
    source.check().unwrap();
    let bytes = export_idml(&source);
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    for (directory, tag, spread) in [("Spreads", "Spread", &source.spreads[0]), ("MasterSpreads", "MasterSpread", &source.parents[0])] {
        let path = format!("{directory}/{tag}_u{:x}.xml", spread.id.0);
        let mut part = Vec::new();
        archive.by_name(&path).unwrap().read_to_end(&mut part).unwrap();
        let root = xml::parse(&part).unwrap();
        let pages: Vec<_> = root.find(tag).unwrap().find_all("Page").collect();
        assert_eq!(pages.len(), spread.pages.len());
        for (xml_page, page) in pages.iter().zip(&spread.pages) {
            let guides: Vec<_> = xml_page.find_all("Guide").collect();
            assert_eq!(guides.len(), page.guides.len());
            for (guide, expected) in guides.iter().zip(&page.guides) {
                assert_eq!(guide.get("GuideType"), Some(if expected.liquid { "Liquid" } else { "Ruler" }), "{path}");
            }
        }
    }
    let actual = import_idml(&bytes).unwrap();
    actual.check().unwrap();
    assert_eq!((actual.spreads.len(), actual.parents.len()), (1, 1));
    for (got, want) in actual.spreads.iter().chain(&actual.parents).zip(source.spreads.iter().chain(&source.parents)) {
        assert_eq!(got.pages.len(), want.pages.len());
        for (got, want) in got.pages.iter().zip(&want.pages) {
            assert_eq!(got.liquid, want.liquid);
            assert_eq!(got.guides.len(), want.guides.len());
            for (got, want) in got.guides.iter().zip(&want.guides) {
                assert_eq!(
                    (got.orientation, got.position, got.spread, got.locked, got.liquid),
                    (want.orientation, want.position, want.spread, want.locked, want.liquid)
                );
                assert_eq!(actual.layer(got.layer.unwrap()).unwrap().name, source.layer(want.layer.unwrap()).unwrap().name);
            }
        }
    }
}
