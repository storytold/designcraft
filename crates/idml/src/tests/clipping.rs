//! Image clipping paths: `ClippingPathSettings` with saved geometry.

use std::sync::Arc;

use designcraft_doc::{ClippingKind, ClippingPath, Content, Graphic};
use designcraft_geom::Point;

use super::*;

/// A 40×30 frame showing an image whose inner space is centred on its origin (GraphicBounds
/// -20,-15 … 20,15), clipped by a triangle with its apex at the top middle.
fn package(settings: &str) -> Vec<u8> {
    let spread = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <Spread Self="sp1" PageCount="1" ItemTransform="1 0 0 1 0 0">
    <Page Self="p1" AppliedMaster="m1" GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 0 0"/>
    <Rectangle Self="r" ItemLayer="L1" ItemTransform="1 0 0 1 100 100" ContentType="GraphicType">
      <Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
        <PathPointType Anchor="0 0"/><PathPointType Anchor="0 30"/><PathPointType Anchor="40 30"/><PathPointType Anchor="40 0"/>
      </PathPointArray></GeometryPathType></PathGeometry></Properties>
      <Image Self="im" ItemTransform="1 0 0 1 20 15">
        <Properties><GraphicBounds Left="-20" Top="-15" Right="20" Bottom="15"/></Properties>
        {settings}
        <Link Self="lk" LinkResourceURI="file:/missing/photo.tif" LinkResourceFormat="$ID/TIFF" StoredState="Normal"/>
      </Image>
    </Rectangle>
  </Spread>
</idPkg:Spread>"#
    );
    zip_files(&[
        ("designmap.xml", DESIGNMAP),
        ("Resources/Graphic.xml", GRAPHIC),
        ("Resources/Styles.xml", STYLES),
        ("Resources/Preferences.xml", PREFS),
        ("MasterSpreads/MasterSpread_m1.xml", MASTER),
        ("Spreads/Spread_sp1.xml", &spread),
        ("Stories/Story_s1.xml", STORY),
    ])
}

const TRIANGLE: &str = r#"<ClippingPathSettings ClippingType="PhotoshopPath" InvertPath="false" IncludeInsideEdges="false" RestrictToFrame="false"
        UseHighResolutionImage="true" Threshold="25" Tolerance="2" InsetFrame="0" AppliedPathName="Path 1" Index="-1">
          <Properties><ClippingPathGeometry>
            <GeometryPathType PathOpen="false"><PathPointArray>
              <PathPointType Anchor="-20 15" LeftDirection="-20 15" RightDirection="-20 15"/>
              <PathPointType Anchor="0 -15" LeftDirection="0 -15" RightDirection="0 -15"/>
              <PathPointType Anchor="20 15" LeftDirection="20 15" RightDirection="20 15"/>
            </PathPointArray></GeometryPathType>
            <GeometryPathType PathOpen="false"><PathPointArray>
              <PathPointType Anchor="NaN 0"/><PathPointType Anchor="1 1"/><PathPointType Anchor="2 0"/>
            </PathPointArray></GeometryPathType>
          </ClippingPathGeometry></Properties>
        </ClippingPathSettings>"#;

fn graphic(d: &Document) -> Graphic {
    d.spreads[0].items.iter().find_map(|i| i.graphic()).unwrap().clone()
}

fn anchors(c: &ClippingPath) -> Vec<Point> {
    c.path.subpaths.iter().flat_map(|s| s.anchors.iter().map(|a| a.p)).collect()
}

#[test]
fn imports_the_saved_clipping_path_in_graphic_space() {
    let d = import_idml(&package(TRIANGLE)).unwrap();
    let g = graphic(&d);
    let c = g.clip.as_ref().unwrap();
    assert_eq!(c.kind, ClippingKind::PhotoshopPath);
    assert_eq!(c.source_name, "Path 1");
    // The subpath with a NaN anchor is dropped; the triangle moves by -GraphicBounds' top left.
    assert_eq!(anchors(c), [Point::new(0.0, 30.0), Point::new(20.0, 0.0), Point::new(40.0, 30.0)]);
    // In the frame: the image fills it, so the apex is at the top middle.
    let (region, even_odd) = g.clip_region().unwrap();
    assert!(!even_odd);
    let b = designcraft_geom::Shape::bounding_box(&region);
    assert!((b.x0 - 0.0).abs() < 1e-9 && (b.x1 - 40.0).abs() < 1e-9 && b.y0.abs() < 1e-9 && (b.y1 - 30.0).abs() < 1e-9, "{b:?}");

    // Type None, or no geometry: no clipping path.
    let none = TRIANGLE.replace("PhotoshopPath", "None");
    assert!(graphic(&import_idml(&package(&none)).unwrap()).clip.is_none());
    assert!(graphic(&import_idml(&package(r#"<ClippingPathSettings ClippingType="DetectEdges"/>"#)).unwrap()).clip.is_none());
}

#[test]
fn clipping_path_round_trips() {
    let mut d = import_idml(&package(&TRIANGLE.replace(r#"InvertPath="false""#, r#"InvertPath="true""#))).unwrap();
    // A user-modified path with a threshold survives too.
    let id = d.spreads[0].items.iter().find(|i| i.graphic().is_some()).unwrap().id;
    if let Content::Graphic(g) = &mut d.item_mut(id).unwrap().content {
        let c = Arc::make_mut(g.clip.as_mut().unwrap());
        c.kind = ClippingKind::UserModifiedPath;
        c.threshold = 40.0;
    }
    let before = graphic(&d);
    let back = graphic(&import_idml(&export_idml(&d)).unwrap());
    let (a, b) = (before.clip.clone().unwrap(), back.clip.clone().unwrap());
    assert!(b.invert);
    assert_eq!(b.kind, ClippingKind::UserModifiedPath);
    assert_eq!(b.threshold, 40.0);
    assert_eq!(anchors(&a), anchors(&b));
    assert!(back.clip_region().unwrap().1, "an inverted clip is filled even-odd");
}
