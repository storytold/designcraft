use designcraft_doc::PageSide::{self, Left, Right};

use super::*;

const NS: &str = r#"xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0""#;

fn rect_item(id: &str, x0: f64, x1: f64) -> String {
    let pts: String = [(x0, -280.0), (x0, -260.0), (x1, -260.0), (x1, -280.0)]
        .iter()
        .map(|(x, y)| format!(r#"<PathPointType Anchor="{x} {y}" LeftDirection="{x} {y}" RightDirection="{x} {y}"/>"#))
        .collect();
    format!(
        r#"<Rectangle Self="{id}" ItemLayer="L1" ItemTransform="1 0 0 1 0 0"><Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>{pts}</PathPointArray></GeometryPathType></PathGeometry></Properties></Rectangle>"#
    )
}

/// Bound right to left, the first page of every spread (parent spreads too) lies right of the
/// spine and the second left of it. The parent's right page holds a 100 pt wide item, its left
/// page a 50 pt one. Five pages: 1 alone, then 2–3 and 4–5.
fn right_to_left_fixture() -> Vec<u8> {
    let page = |id: &str, x: f64, master: bool| {
        let applied = if master { String::new() } else { r#" AppliedMaster="m1""#.to_string() };
        format!(r#"<Page Self="{id}"{applied} GeometricBounds="0 0 600 400" ItemTransform="1 0 0 1 {x} -300"/>"#)
    };
    let master = format!(
        r#"<idPkg:MasterSpread {NS}><MasterSpread Self="m1" Name="A-Parent" NamePrefix="A" BaseName="Parent" PageCount="2" ItemTransform="1 0 0 1 0 0">{}{}{}{}</MasterSpread></idPkg:MasterSpread>"#,
        page("mp1", 0.0, true),
        page("mp2", -400.0, true),
        rect_item("right", 50.0, 150.0),
        rect_item("left", -350.0, -300.0),
    );
    let spread = |id: &str, pages: &[(&str, f64)]| {
        let ps: String = pages.iter().map(|(p, x)| page(p, *x, false)).collect();
        format!(r#"<idPkg:Spread {NS}><Spread Self="{id}" PageCount="{}" ItemTransform="1 0 0 1 0 0">{ps}</Spread></idPkg:Spread>"#, pages.len())
    };
    let map = format!(
        r#"<Document {NS} Self="d"><idPkg:Preferences src="Resources/Preferences.xml"/><Layer Self="L1" Name="Layer 1"/><idPkg:MasterSpread src="MasterSpreads/m1.xml"/><idPkg:Spread src="Spreads/sp0.xml"/><idPkg:Spread src="Spreads/sp1.xml"/><idPkg:Spread src="Spreads/sp2.xml"/></Document>"#
    );
    let prefs = format!(
        r#"<idPkg:Preferences {NS}><DocumentPreference PageWidth="400" PageHeight="600" FacingPages="true" PageBinding="RightToLeft"/></idPkg:Preferences>"#
    );
    zip_files(&[
        ("designmap.xml", &map),
        ("Resources/Preferences.xml", &prefs),
        ("MasterSpreads/m1.xml", &master),
        ("Spreads/sp0.xml", &spread("sp0", &[("p1", 0.0)])),
        ("Spreads/sp1.xml", &spread("sp1", &[("p2", 0.0), ("p3", -400.0)])),
        ("Spreads/sp2.xml", &spread("sp2", &[("p4", 0.0), ("p5", -400.0)])),
    ])
}

/// The parent items a document page shows, as (width, bounds on the document spread).
fn shown_parent_items(d: &Document, abs: usize) -> Vec<(f64, Rect)> {
    let (si, pi) = d.page_loc(abs).unwrap();
    let page = &d.spreads[si].pages[pi];
    let (ppi, ppage) = d.parent_page_for(abs).unwrap();
    let parent = &d.parents[ppi];
    let dx = page.x - parent.pages[ppage].x;
    parent
        .items
        .iter()
        .filter(|it| parent.page_at_x(it.bounds().center().x) == Some(ppage))
        .map(|it| {
            let b = it.bounds();
            (b.width(), Rect::new(b.x0 + dx, b.y0, b.x1 + dx, b.y1))
        })
        .collect()
}

fn assert_parent_items_follow_page_sides(d: &Document) {
    assert!(d.settings.right_to_left_binding);
    let sides: Vec<PageSide> = d.parents[0].pages.iter().map(|p| p.side).collect();
    assert_eq!(sides, vec![Right, Left], "the parent spread lists its right page first");
    for (abs, side, width) in [(0, Right, 100.0), (1, Right, 100.0), (2, Left, 50.0), (3, Right, 100.0), (4, Left, 50.0)] {
        let page = d.page(abs).unwrap();
        assert_eq!(page.side, side, "page {}", abs + 1);
        let shown = shown_parent_items(d, abs);
        assert_eq!(shown.len(), 1, "page {}", abs + 1);
        let (w, b) = shown[0];
        assert!((w - width).abs() < 1e-6, "page {} shows the {width} pt item, not {w}", abs + 1);
        assert!(b.x0 >= page.x - 1e-6 && b.x1 <= page.x + page.width + 1e-6, "page {}: {b:?} lies on the page", abs + 1);
    }
}

#[test]
fn right_to_left_parent_pages_follow_the_document_page_side() {
    let d = import_idml(&right_to_left_fixture()).unwrap();
    assert_parent_items_follow_page_sides(&d);
    let again = import_idml(&export_idml(&d)).unwrap();
    assert_parent_items_follow_page_sides(&again);
}
