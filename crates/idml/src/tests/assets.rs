//! Placed files: every placement of one file shares an asset.

use std::cell::Cell;

use designcraft_doc::{AssetId, Content};

use super::*;

const PHOTO: &str = "file:/photos/wedding%20day/couple.jpg";

/// A graphic frame at `x` showing `image` (an `<Image>` element) with `crop` on every side.
fn frame(n: usize, x: f64, crop: f64, image: &str) -> String {
    format!(
        r#"<Rectangle Self="r{n}" ItemLayer="L1" ItemTransform="1 0 0 1 {x} 0" ContentType="GraphicType">
      <Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
        <PathPointType Anchor="0 0"/><PathPointType Anchor="0 30"/><PathPointType Anchor="40 30"/><PathPointType Anchor="40 0"/>
      </PathPointArray></GeometryPathType></PathGeometry></Properties>
      <FrameFittingOption TopCrop="{crop}" LeftCrop="{crop}" BottomCrop="{crop}" RightCrop="{crop}"/>
      {image}
    </Rectangle>"#
    )
}

fn image(n: usize, uri: &str, contents: Option<&[u8]>) -> String {
    let contents = contents.map(|c| format!("<Contents><![CDATA[{}]]></Contents>", base64_encode(c))).unwrap_or_default();
    format!(
        r#"<Image Self="im{n}" ItemTransform="1 0 0 1 {n} 0">
        <Properties>{contents}<GraphicBounds Left="0" Top="0" Right="40" Bottom="30"/></Properties>
        <Link Self="lk{n}" LinkResourceURI="{uri}" LinkResourceFormat="$ID/JPEG" StoredState="Normal"/>
      </Image>"#
    )
}

fn package(frames: &[String]) -> Vec<u8> {
    let spread = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0">
  <Spread Self="sp1" PageCount="1" ItemTransform="1 0 0 1 0 0">
    <Page Self="p1" AppliedMaster="m1" GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 0 0"/>
    {}
  </Spread>
</idPkg:Spread>"#,
        frames.join("\n")
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

/// (asset, crop, transform) of each graphic, left to right.
fn placements(d: &Document) -> Vec<(AssetId, [f64; 4], designcraft_geom::Affine)> {
    let mut out: Vec<(f64, AssetId, [f64; 4], designcraft_geom::Affine)> = d.spreads[0]
        .items
        .iter()
        .filter_map(|i| match &i.content {
            Content::Graphic(g) => Some((i.xf.translation().x, g.asset, g.crop, g.xf)),
            _ => None,
        })
        .collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out.into_iter().map(|(_, a, c, x)| (a, c, x)).collect()
}

#[test]
fn one_file_placed_several_times_imports_as_one_asset() {
    let bytes = vec![42u8; 300];
    let reads = Cell::new(0);
    let reader = |p: &str| {
        reads.set(reads.get() + 1);
        p.ends_with("couple.jpg").then(|| bytes.clone())
    };
    // Three placements linking the file, and one embedding the same bytes under another path.
    let frames = vec![
        frame(1, 0.0, 1.0, &image(1, PHOTO, None)),
        frame(2, 100.0, 2.0, &image(2, PHOTO, None)),
        frame(3, 200.0, 3.0, &image(3, PHOTO, None)),
        frame(4, 300.0, 4.0, &image(4, "file:/elsewhere/copy.jpg", Some(&bytes))),
    ];
    let d = import_idml_with(&package(&frames), &reader).unwrap();
    assert_eq!(reads.get(), 1, "the linked file is read once");
    assert_eq!(d.assets.len(), 1);
    let a = d.assets.values().next().unwrap();
    assert_eq!(a.link.as_deref(), Some("/photos/wedding day/couple.jpg"));
    let p = placements(&d);
    assert_eq!(p.iter().map(|(_, c, _)| c[0]).collect::<Vec<_>>(), vec![1.0, 2.0, 3.0, 4.0]);
    assert!(p.iter().all(|(id, _, _)| *id == a.id));

    // Exported linked or embedded, the round trip keeps one asset and each placement's crop and
    // transform.
    for embed_images in [false, true] {
        let out = export_idml_with(&d, &ExportOptions { embed_images });
        let back = import_idml_with(&out, &reader).unwrap();
        assert_eq!(back.assets.len(), 1, "embed_images: {embed_images}");
        let q = placements(&back);
        assert_eq!(q.len(), 4);
        for ((_, c0, x0), (_, c1, x1)) in p.iter().zip(&q) {
            assert_eq!(c0, c1);
            assert!(x0.as_coeffs().iter().zip(x1.as_coeffs()).all(|(a, b)| (a - b).abs() < 1e-6), "{x0:?} {x1:?}");
        }
    }
}
