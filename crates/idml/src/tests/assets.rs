//! Placed files: every placement of one file shares an asset.

use std::cell::Cell;

use designcraft_doc::{AssetId, Content, PdfCrop};

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

/// A graphic frame showing `insert.pdf` (a `<PDF>` element) at page `PageNumber` (1-based),
/// cropped to `crop` (`PDFCrop`).
fn pdf_frame(n: usize, x: f64, page: u32, crop: &str) -> String {
    format!(
        r#"<Rectangle Self="r{n}" ItemLayer="L1" ItemTransform="1 0 0 1 {x} 0" ContentType="GraphicType">
      <Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
        <PathPointType Anchor="0 0"/><PathPointType Anchor="0 30"/><PathPointType Anchor="40 30"/><PathPointType Anchor="40 0"/>
      </PathPointArray></GeometryPathType></PathGeometry></Properties>
      <PDF Self="im{n}" ItemTransform="1 0 0 1 {n} 0">
        <Properties><GraphicBounds Left="0" Top="0" Right="40" Bottom="30"/></Properties>
        <Link Self="lk{n}" LinkResourceURI="file:/art/insert.pdf" LinkResourceFormat="$ID/PDF" StoredState="Normal"/>
        <PDFAttribute PageNumber="{page}" PDFCrop="{crop}" TransparentBackground="true"/>
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

#[test]
fn nested_graphic_layers_preserve_hierarchy_on_roundtrip() {
    let option = r#"<GraphicLayerOption UpdateLinkOption="KeepOverrides">
      <GraphicLayer Self="a" Id="1" Name="Group"><GraphicLayer Self="b" Id="2" Name="Nested" OriginalVisibility="true" CurrentVisibility="false"/></GraphicLayer>
      <GraphicLayer Self="c" Id="3" Name="Sibling"/>
    </GraphicLayerOption>"#;
    let image = image(1, PHOTO, None).replace("</Image>", &format!("{option}</Image>"));
    let d = import_idml(&package(&[frame(1, 0.0, 0.0, &image)])).unwrap();
    let options = |d: &Document| d.spreads[0].items[0].graphic().unwrap().layer_options.clone().unwrap();
    let expected = options(&d);
    assert_eq!(expected.layers.len(), 3);
    assert_eq!(expected.parents, [None, Some(0), None]);
    assert!(expected.has_visibility_overrides());
    let reopened = import_idml(&export_idml(&d)).unwrap();
    assert_eq!(options(&reopened), expected);
}

#[test]
fn graphic_layer_options_survive_for_each_placement_of_a_shared_asset() {
    let option = |n: usize, visible: &str| {
        format!(
            r#"<GraphicLayerOption UpdateLinkOption="KeepOverrides"><GraphicLayer Self="gl{n}" Id="7" Name="Synthetic layer" OriginalVisibility="true" CurrentVisibility="{visible}" Locked="true" HasPrintState="true" PrintState="false"/></GraphicLayerOption>"#
        )
    };
    let frames = ["false", "true"]
        .into_iter()
        .enumerate()
        .map(|(i, visible)| {
            let n = i + 1;
            frame(n, i as f64 * 100.0, 0.0, &image(n, PHOTO, None).replace("</Image>", &format!("{}</Image>", option(n, visible))))
        })
        .collect::<Vec<_>>();
    let reads = Cell::new(0);
    let loaded = import_idml_report_with_previews(
        &package(&frames),
        &|_| {
            reads.set(reads.get() + 1);
            Some(vec![42; 16])
        },
        &Default::default(),
    )
    .unwrap();
    assert_eq!(reads.get(), 1);
    assert_eq!(loaded.document.assets.len(), 1, "layer choices belong to placements, not shared image bytes");
    let out = export_idml(&loaded.document);
    let spreads = zip_text(&out, "Spreads/");
    assert_eq!(spreads.matches("<GraphicLayer ").count(), 2, "both placements must retain their independent layer choices");
    assert!(spreads.contains("CurrentVisibility=\"false\""));
    assert!(spreads.contains("CurrentVisibility=\"true\""));
    assert_eq!(spreads.matches("UpdateLinkOption=\"KeepOverrides\"").count(), 2);
    assert_eq!(spreads.matches("PrintState=\"false\"").count(), 2);
    assert!(loaded.warnings.iter().any(|w| w.contains("layer visibility")));
    let reopened = import_idml(&out).unwrap();
    let options = |d: &Document| {
        d.spreads[0]
            .items
            .iter()
            .filter_map(|i| match &i.content {
                Content::Graphic(g) => g.layer_options.clone(),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(options(&loaded.document), options(&reopened));
    let twice = zip_text(&export_idml(&reopened), "Spreads/");
    assert_eq!(twice.matches("<GraphicLayer ").count(), 2);
}

#[test]
fn malformed_native_graphic_layer_parents_cannot_cycle_or_overflow_xml_depth() {
    let mut d = import_idml(&package(&[frame(1, 0.0, 0.0, &image(1, PHOTO, None))])).unwrap();
    let Content::Graphic(g) = &mut std::sync::Arc::make_mut(&mut std::sync::Arc::make_mut(&mut d.spreads[0]).items[0]).content else {
        panic!("graphic")
    };
    let mut options = designcraft_doc::GraphicLayerOptions::default();
    for i in 0..300usize {
        options.layers.push(std::collections::BTreeMap::from([("Name".into(), format!("Layer {i}"))]));
        options.parents.push(Some(i.saturating_sub(1)));
    }
    options.parents[1] = Some(usize::MAX);
    g.layer_options = Some(options);
    let reopened = import_idml(&export_idml(&d)).unwrap();
    assert_eq!(reopened.spreads[0].items[0].graphic().unwrap().layer_options.as_ref().unwrap().layers.len(), 300);
}

#[test]
fn closed_graphic_layer_subtrees_cannot_reorder_native_layers() {
    let mut d = import_idml(&package(&[frame(1, 0.0, 0.0, &image(1, PHOTO, None))])).unwrap();
    let Content::Graphic(g) = &mut std::sync::Arc::make_mut(&mut std::sync::Arc::make_mut(&mut d.spreads[0]).items[0]).content else {
        panic!("graphic")
    };
    g.layer_options = Some(designcraft_doc::GraphicLayerOptions {
        layers: ["A", "B", "C", "D"].into_iter().map(|name| std::collections::BTreeMap::from([("Name".into(), name.into())])).collect(),
        parents: vec![None, None, Some(0), Some(2)],
        ..Default::default()
    });
    let reopened = import_idml(&export_idml(&d)).unwrap();
    let options = reopened.spreads[0].items[0].graphic().unwrap().layer_options.as_ref().unwrap();
    assert_eq!(options.layers.iter().map(|a| a["Name"].as_str()).collect::<Vec<_>>(), ["A", "B", "C", "D"]);
    assert_eq!(options.parents, [None, None, None, Some(2)]);
}

#[test]
fn graphic_layer_attributes_from_native_data_cannot_inject_xml_or_self_ids() {
    let mut d = import_idml(&package(&[frame(1, 0.0, 0.0, &image(1, PHOTO, None))])).unwrap();
    let Content::Graphic(g) = &mut std::sync::Arc::make_mut(&mut std::sync::Arc::make_mut(&mut d.spreads[0]).items[0]).content else {
        panic!("graphic")
    };
    let attributes = std::collections::BTreeMap::from([
        ("Name".into(), "A < B & C".into()),
        ("Self".into(), "injected-id".into()),
        ("xmlns".into(), "urn:injected".into()),
        ("bad\" injected=\"true".into(), "invalid".into()),
    ]);
    g.layer_options = Some(designcraft_doc::GraphicLayerOptions { attributes: attributes.clone(), layers: vec![attributes], ..Default::default() });
    let out = export_idml(&d);
    let xml = zip_text(&out, "Spreads/");
    assert!(!xml.contains("injected-id") && !xml.contains("injected=") && !xml.contains("urn:injected"));
    let back = import_idml(&out).unwrap();
    let Content::Graphic(g) = &back.spreads[0].items[0].content else { panic!("graphic") };
    assert_eq!(g.layer_options.as_ref().unwrap().layers[0]["Name"], "A < B & C");
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
    let frames = vec![pdf_frame(1, 0.0, 2, "CropMedia"), pdf_frame(2, 100.0, 1, "CropMedia"), pdf_frame(3, 200.0, 2, "CropMedia")];
    let d = import_idml_with(&package(&frames), &reader).unwrap();
    let p = placements(&d);
    let page = |id| d.assets.get(&id).map(|a| a.page);
    assert_eq!(page(p[0].0), Some(1), "PageNumber=\"2\" is the second page, stored 0-based");
    assert_eq!(page(p[1].0), Some(0), "PageNumber=\"1\" is the first page");
    assert_eq!(p[0].0, p[2].0, "the same file at the same page shares one asset");
    assert_ne!(p[0].0, p[1].0, "the same file at another page is another asset");

    // Exported (linked or embedded), every placement writes its page and crop, page 1 included,
    // and both survive the round trip.
    for embed_images in [false, true] {
        let out = export_idml_with(&d, &ExportOptions { embed_images });
        let spreads = super::zip_text(&out, "Spreads/");
        assert_eq!(spreads.matches(r#"PDFCrop="CropMedia""#).count(), 3, "{spreads}");
        assert!(spreads.contains(r#"PageNumber="1""#) && spreads.contains(r#"PageNumber="2""#), "{spreads}");
        let back = import_idml_with(&out, &reader).unwrap();
        let shown: Vec<_> = placements(&back).iter().map(|(id, _, _)| back.assets.get(id).map(|a| (a.page, a.pdf_crop))).collect();
        let (first, second) = (Some((0, PdfCrop::Media)), Some((1, PdfCrop::Media)));
        assert_eq!(shown, vec![second, first, second], "embed_images: {embed_images}");
    }
}

/// Two crops of one linked PDF page are two assets, so each frame keeps its own crop; the same
/// page and crop placed again shares an asset, and the file is read once per page and crop.
#[test]
fn a_pdf_page_placed_with_two_crops_imports_as_two_assets() {
    let bytes = b"%PDF-1.7\n% fixture".to_vec();
    let reads = Cell::new(0);
    let reader = |p: &str| {
        reads.set(reads.get() + 1);
        p.ends_with("insert.pdf").then(|| bytes.clone())
    };
    let frames = vec![pdf_frame(1, 0.0, 1, "CropTrim"), pdf_frame(2, 100.0, 1, "CropMedia"), pdf_frame(3, 200.0, 1, "CropTrim")];
    let d = import_idml_with(&package(&frames), &reader).unwrap();
    let p = placements(&d);
    let crop = |id| d.assets.get(&id).map(|a| (a.page, a.pdf_crop));
    assert_eq!(crop(p[0].0), Some((0, PdfCrop::Trim)));
    assert_eq!(crop(p[1].0), Some((0, PdfCrop::Media)));
    assert_ne!(p[0].0, p[1].0, "another crop of the same page is another asset");
    assert_eq!(p[0].0, p[2].0, "the same page and crop share one asset");
    assert_eq!(d.assets.len(), 2);
    assert_eq!(reads.get(), 2);
}

/// The swatch applied to a placed image (`<Image FillColor FillTint>`) colours a greyscale or
/// bitmap image in InDesign (#1094). Black at 100%, InDesign's own value for an image nobody
/// coloured, and None leave the image as it is, like a missing attribute.
#[test]
fn the_swatch_applied_to_an_image_is_kept() {
    let bytes = vec![42u8; 300];
    let reader = |p: &str| p.ends_with("couple.jpg").then(|| bytes.clone());
    let coloured = |n: usize, attrs: &str| image(n, PHOTO, None).replacen(&format!(r#"Self="im{n}""#), &format!(r#"Self="im{n}" {attrs}"#), 1);
    let frames = vec![
        frame(1, 0.0, 0.0, &coloured(1, r#"FillColor="Color/Brand" FillTint="50""#)),
        frame(2, 100.0, 0.0, &coloured(2, r#"FillColor="Color/Black" FillTint="-1""#)),
        frame(3, 200.0, 0.0, &coloured(3, r#"FillColor="Swatch/None""#)),
        frame(4, 300.0, 0.0, &image(4, PHOTO, None)),
        frame(5, 400.0, 0.0, &coloured(5, r#"FillColor="Color/u9""#)),
        frame(6, 500.0, 0.0, &coloured(6, r#"FillColor="Color/Black" FillTint="40""#)),
    ];
    let fills = |d: &Document| {
        let mut v: Vec<(f64, (String, f32))> = d.spreads[0]
            .items
            .iter()
            .filter_map(|i| match &i.content {
                Content::Graphic(g) => Some((i.xf.translation().x, (g.fill.swatch.clone(), g.fill.tint))),
                _ => None,
            })
            .collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        v.into_iter().map(|(_, f)| f).collect::<Vec<_>>()
    };
    let none = || (designcraft_color::swatch::NONE.to_string(), 1.0);
    let want = vec![
        ("Brand".to_string(), 0.5),
        none(),
        none(),
        none(),
        ("C=10 M=20 Y=30 K=40".to_string(), 1.0),
        (designcraft_color::swatch::BLACK.to_string(), 0.4),
    ];
    let d = import_idml_with(&package(&frames), &reader).unwrap();
    assert_eq!(fills(&d), want);

    let out = export_idml_with(&d, &ExportOptions::default());
    let spreads = zip_text(&out, "Spreads/");
    let images: Vec<&str> = spreads.split("<Image ").skip(1).map(|s| s.split('>').next().unwrap_or("")).collect();
    assert_eq!(images.len(), 6);
    // Only an image with a swatch says so; none of them gets an invented `Swatch/None`.
    assert_eq!(images.iter().filter(|e| e.contains("FillColor=")).count(), 3, "{images:?}");
    assert!(images.iter().any(|e| e.contains(r#"FillColor="Color/Brand""#) && e.contains(r#"FillTint="50""#)), "{images:?}");
    assert!(!images.iter().any(|e| e.contains("Swatch/None")), "{images:?}");
    assert_eq!(fills(&import_idml_with(&out, &reader).unwrap()), want);
}
