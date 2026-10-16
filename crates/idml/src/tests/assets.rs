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

/// A graphic frame showing `pdf` (a `<PDF>` element) with the page `PageNumber` (1-based).
fn pdf_frame(n: usize, x: f64, page: u32) -> String {
    format!(
        r#"<Rectangle Self="r{n}" ItemLayer="L1" ItemTransform="1 0 0 1 {x} 0" ContentType="GraphicType">
      <Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
        <PathPointType Anchor="0 0"/><PathPointType Anchor="0 30"/><PathPointType Anchor="40 30"/><PathPointType Anchor="40 0"/>
      </PathPointArray></GeometryPathType></PathGeometry></Properties>
      <PDF Self="im{n}" ItemTransform="1 0 0 1 {n} 0">
        <Properties><GraphicBounds Left="0" Top="0" Right="40" Bottom="30"/></Properties>
        <Link Self="lk{n}" LinkResourceURI="file:/art/insert.pdf" LinkResourceFormat="$ID/PDF" StoredState="Normal"/>
        <PDFAttribute PageNumber="{page}" PDFCrop="CropMedia" TransparentBackground="true"/>
      </PDF>
    </Rectangle>"#
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
    // Three placements linking the file, and one embedding the same bytes under another path:
    // that one keeps its own asset and path.
    let frames = vec![
        frame(1, 0.0, 1.0, &image(1, PHOTO, None)),
        frame(2, 100.0, 2.0, &image(2, PHOTO, None)),
        frame(3, 200.0, 3.0, &image(3, PHOTO, None)),
        frame(4, 300.0, 4.0, &image(4, "file:/elsewhere/copy.jpg", Some(&bytes))),
    ];
    let d = import_idml_with(&package(&frames), &reader).unwrap();
    assert_eq!(reads.get(), 1, "the linked file is read once");
    assert_eq!(d.assets.len(), 2);
    let p = placements(&d);
    assert_eq!(p.iter().map(|(_, c, _)| c[0]).collect::<Vec<_>>(), vec![1.0, 2.0, 3.0, 4.0]);
    let link = |id| d.assets.get(id).and_then(|a| a.link.as_deref());
    let links: Vec<_> = p.iter().map(|(id, _, _)| link(id)).collect();
    assert_eq!(links[..3], [Some("/photos/wedding day/couple.jpg"); 3]);
    assert_eq!(links[3], Some("/elsewhere/copy.jpg"));
    assert!(p[..3].iter().all(|(id, _, _)| *id == p[0].0));

    // Exported linked or embedded, the round trip keeps the two assets and each placement's crop
    // and transform.
    for embed_images in [false, true] {
        let out = export_idml_with(&d, &ExportOptions { embed_images });
        let back = import_idml_with(&out, &reader).unwrap();
        assert_eq!(back.assets.len(), 2, "embed_images: {embed_images}");
        let q = placements(&back);
        assert_eq!(q.len(), 4);
        for ((_, c0, x0), (_, c1, x1)) in p.iter().zip(&q) {
            assert_eq!(c0, c1);
            assert!(x0.as_coeffs().iter().zip(x1.as_coeffs()).all(|(a, b)| (a - b).abs() < 1e-6), "{x0:?} {x1:?}");
        }
    }
}

/// An `<SVG>` placement linking `uri`, whose GraphicBounds are `bounds` (left top right bottom).
fn svg(n: usize, uri: &str, bounds: [f64; 4]) -> String {
    let [l, t, r, b] = bounds;
    format!(
        r#"<SVG Self="sv{n}" ItemTransform="1 0 0 1 0 0">
        <Properties><GraphicBounds Left="{l}" Top="{t}" Right="{r}" Bottom="{b}"/></Properties>
        <Link Self="lk{n}" LinkResourceURI="{uri}" LinkResourceFormat="$ID/SVG" StoredState="Normal"/>
      </SVG>"#
    )
}

#[test]
fn placed_svg_with_only_a_viewbox_keeps_its_viewbox_size() {
    const OVERHANG: &str =
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100"><line x1="-10" y1="50" x2="0" y2="50" stroke="#000"/></svg>"##;
    const SIZED: &str =
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 200 100"><rect width="200" height="100"/></svg>"#;
    let reader = |p: &str| match p {
        "/art/overhang.svg" => Some(OVERHANG.as_bytes().to_vec()),
        "/art/sized.svg" => Some(SIZED.as_bytes().to_vec()),
        _ => None,
    };
    // GraphicBounds are the art's bounds: 211 wide for the overhanging one, placed twice (the second
    // reuses the first's asset); a sized SVG is still stretched onto its bounds.
    let frames = vec![
        frame(1, 0.0, 0.0, &svg(1, "file:/art/overhang.svg", [-11.0, 0.0, 200.0, 100.0])),
        frame(2, 100.0, 0.0, &svg(2, "file:/art/overhang.svg", [-11.0, 0.0, 200.0, 100.0])),
        frame(3, 200.0, 0.0, &svg(3, "file:/art/sized.svg", [-10.0, 0.0, 200.0, 100.0])),
    ];
    let d = import_idml_with(&package(&frames), &reader).unwrap();
    let mut gs: Vec<_> = d.spreads[0]
        .items
        .iter()
        .filter_map(|i| match &i.content {
            Content::Graphic(g) => Some((i.xf.translation().x, g.size, g.xf.translation().x)),
            _ => None,
        })
        .collect();
    gs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let got: Vec<_> = gs.into_iter().map(|(_, size, x)| (size, x)).collect();
    assert_eq!(got, vec![((200.0, 100.0), -11.0), ((200.0, 100.0), -11.0), ((210.0, 100.0), -10.0)]);
}

/// `PDFAttribute/@PageNumber` (1-based) is read into `Asset.page` (0-based), and the same file at
/// different pages stays apart (#905).
#[test]
fn a_pdf_placement_reads_its_page_number() {
    let bytes = b"%PDF-1.7\n% fixture".to_vec();
    let reader = |p: &str| p.ends_with("insert.pdf").then(|| bytes.clone());
    let frames = vec![pdf_frame(1, 0.0, 2), pdf_frame(2, 100.0, 1), pdf_frame(3, 200.0, 2)];
    let d = import_idml_with(&package(&frames), &reader).unwrap();
    let p = placements(&d);
    let page = |id| d.assets.get(&id).map(|a| a.page);
    assert_eq!(page(p[0].0), Some(1), "PageNumber=\"2\" is the second page, stored 0-based");
    assert_eq!(page(p[1].0), Some(0), "PageNumber=\"1\" is the first page");
    assert_eq!(p[0].0, p[2].0, "the same file at the same page shares one asset");
    assert_ne!(p[0].0, p[1].0, "the same file at another page is another asset");

    // Exported (linked or embedded), the page survives the round trip.
    for embed_images in [false, true] {
        let out = export_idml_with(&d, &ExportOptions { embed_images });
        let back = import_idml_with(&out, &reader).unwrap();
        let pages: Vec<_> = placements(&back).iter().map(|(id, _, _)| back.assets.get(id).map(|a| a.page)).collect();
        assert_eq!(pages, vec![Some(1), Some(0), Some(1)], "embed_images: {embed_images}");
    }
}
