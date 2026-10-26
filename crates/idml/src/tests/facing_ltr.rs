use designcraft_doc::PageSide::{self, Left, Right};

use super::*;

const NS: &str = r#"xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="16.0""#;

/// A running head: a text frame across `x0..x1` (IDML spread x), 20 pt below the page top.
fn head(id: &str, story: &str, x0: f64, x1: f64) -> String {
    let pts: String = [(x0, -280.0), (x0, -260.0), (x1, -260.0), (x1, -280.0)]
        .iter()
        .map(|(x, y)| format!(r#"<PathPointType Anchor="{x} {y}" LeftDirection="{x} {y}" RightDirection="{x} {y}"/>"#))
        .collect();
    format!(
        r#"<TextFrame Self="{id}" ParentStory="{story}" PreviousTextFrame="n" NextTextFrame="n" ItemLayer="L1" ItemTransform="1 0 0 1 0 0"><Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>{pts}</PathPointArray></GeometryPathType></PathGeometry></Properties></TextFrame>"#
    )
}

fn story(id: &str, text: &str) -> String {
    format!(
        r#"<idPkg:Story {NS}><Story Self="{id}"><ParagraphStyleRange><CharacterStyleRange><Content>{text}</Content></CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>"#
    )
}

/// Bound left to right with facing pages: the first page of a spread lies left of the spine
/// (except page 1, alone on the right). Every page has margins `Left=36 Right=120` (inside 36,
/// outside 120), the parent's left page a ruler guide at 108 and a "Left head" running head, its
/// right page a guide at 300 and a "Right head". Three pages: 1 alone, then 2–3. Rulers measure
/// from each page.
fn left_to_right_fixture() -> Vec<u8> {
    let margins = r#"<MarginPreference ColumnCount="1" ColumnGutter="12" Top="36" Bottom="36" Left="36" Right="120"/>"#;
    let page = |id: &str, x: f64, applied: bool, guide: Option<f64>| {
        let applied = if applied { r#" AppliedMaster="m1""# } else { "" };
        let guide = guide.map_or(String::new(), |at| format!(r#"<Guide Self="{id}g" Orientation="Vertical" Location="{at}"/>"#));
        format!(r#"<Page Self="{id}"{applied} GeometricBounds="0 0 600 400" ItemTransform="1 0 0 1 {x} -300">{margins}{guide}</Page>"#)
    };
    let master = format!(
        r#"<idPkg:MasterSpread {NS}><MasterSpread Self="m1" Name="A-Parent" NamePrefix="A" BaseName="Parent" PageCount="2" ItemTransform="1 0 0 1 0 0">{}{}{}{}</MasterSpread></idPkg:MasterSpread>"#,
        page("mp1", -400.0, false, Some(108.0)),
        page("mp2", 0.0, false, Some(300.0)),
        head("hl", "sl", -280.0, -36.0),
        head("hr", "sr", 36.0, 280.0),
    );
    let spread = |id: &str, pages: &[(&str, f64)]| {
        let ps: String = pages.iter().map(|(p, x)| page(p, *x, true, None)).collect();
        format!(r#"<idPkg:Spread {NS}><Spread Self="{id}" PageCount="{}" ItemTransform="1 0 0 1 0 0">{ps}</Spread></idPkg:Spread>"#, pages.len())
    };
    let map = format!(
        r#"<Document {NS} Self="d" StoryList="sl sr"><idPkg:Preferences src="Resources/Preferences.xml"/><Layer Self="L1" Name="Layer 1"/><idPkg:MasterSpread src="MasterSpreads/m1.xml"/><idPkg:Spread src="Spreads/sp0.xml"/><idPkg:Spread src="Spreads/sp1.xml"/><idPkg:Story src="Stories/sl.xml"/><idPkg:Story src="Stories/sr.xml"/></Document>"#
    );
    let prefs = format!(
        r#"<idPkg:Preferences {NS}><DocumentPreference PageWidth="400" PageHeight="600" FacingPages="true" PageBinding="LeftToRight"/><ViewPreference RulerOrigin="PageOrigin"/></idPkg:Preferences>"#
    );
    zip_files(&[
        ("designmap.xml", &map),
        ("Resources/Preferences.xml", &prefs),
        ("MasterSpreads/m1.xml", &master),
        ("Spreads/sp0.xml", &spread("sp0", &[("p1", 0.0)])),
        ("Spreads/sp1.xml", &spread("sp1", &[("p2", -400.0), ("p3", 0.0)])),
        ("Stories/sl.xml", &story("sl", "Left head")),
        ("Stories/sr.xml", &story("sr", "Right head")),
    ])
}

/// The text of the running heads a document page shows from its parent page.
fn shown_heads(d: &Document, abs: usize) -> Vec<String> {
    let (ppi, ppage) = d.parent_page_for(abs).unwrap();
    let parent = &d.parents[ppi];
    parent
        .items
        .iter()
        .filter(|it| parent.page_at_x(it.bounds().center().x) == Some(ppage))
        .filter_map(|it| d.story(it.text_frame()?.story).map(|s| s.text.clone()))
        .collect()
}

/// Ruler guides of the parent page a document page shows, as x from that parent page's left edge.
fn parent_guides(d: &Document, abs: usize) -> Vec<f64> {
    let (ppi, ppage) = d.parent_page_for(abs).unwrap();
    let pp = &d.parents[ppi].pages[ppage];
    pp.guides.iter().map(|g| g.position - pp.x).collect()
}

fn assert_right_pages_mirror_left_pages(d: &Document) {
    assert!(d.settings.facing_pages && !d.settings.right_to_left_binding);
    let sides: Vec<PageSide> = d.parents[0].pages.iter().map(|p| p.side).collect();
    assert_eq!(sides, vec![Left, Right]);
    // Right pages: inside (36) on the left; left pages: outside (120) on the left.
    for (abs, side, live, guide, text) in [
        (0, Right, (36.0, 280.0), 300.0, "Right head"),
        (1, Left, (120.0, 364.0), 108.0, "Left head"),
        (2, Right, (36.0, 280.0), 300.0, "Right head"),
    ] {
        let n = abs + 1;
        let page = d.page(abs).unwrap();
        assert_eq!(page.side, side, "page {n}");
        let m = page.margin_rect();
        assert!((m.x0 - page.x - live.0).abs() < 1e-6 && (m.x1 - page.x - live.1).abs() < 1e-6, "page {n}: margins {m:?}");
        assert_eq!(d.parents[0].pages[d.parent_page_for(abs).unwrap().1].side, side, "page {n} shows the parent's {side:?} page");
        let guides = parent_guides(d, abs);
        assert!(guides.len() == 1 && (guides[0] - guide).abs() < 1e-6, "page {n}: guides {guides:?}");
        assert_eq!(shown_heads(d, abs), vec![text.to_string()], "page {n}");
    }
}

#[test]
fn left_to_right_right_pages_take_mirrored_margins_and_the_right_parent_page() {
    let d = import_idml(&left_to_right_fixture()).unwrap();
    assert_right_pages_mirror_left_pages(&d);
    let again = import_idml(&export_idml(&d)).unwrap();
    assert_right_pages_mirror_left_pages(&again);
}
