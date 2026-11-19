use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

use super::*;
use designcraft_doc::{Content, Item, ItemId, Spread};
use designcraft_geom::Affine;

fn rectangle(id: &str, x: i32, attribute: &str) -> String {
    format!(
        r#"<Rectangle Self="{id}" Name="{id}" ItemLayer="layer" {attribute}>
          <Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
            <PathPointType Anchor="{x} 10"/><PathPointType Anchor="{x} 30"/>
            <PathPointType Anchor="{} 30"/><PathPointType Anchor="{} 10"/>
          </PathPointArray></GeometryPathType></PathGeometry></Properties>
        </Rectangle>"#,
        x + 20,
        x + 20
    )
}

/// Synthetic IDML with no item-level override attributes unless a test explicitly adds one.
/// Three pages share two parent items; the last copy is nested 70 groups deep.
fn fixture(lists: [&str; 3], attribute: &str, extra: &str) -> Vec<u8> {
    let designmap = r#"<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" Self="doc">
      <Layer Self="layer" Name="Art"/>
      <DocumentPreference PageWidth="200" PageHeight="200" FacingPages="true"/>
      <idPkg:MasterSpread src="MasterSpreads/parent.xml"/>
      <idPkg:Spread src="Spreads/first.xml"/><idPkg:Spread src="Spreads/facing.xml"/>
    </Document>"#;
    let parent = format!(
        r#"<idPkg:MasterSpread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
          <MasterSpread Self="master" Name="A-Parent" NamePrefix="A" BaseName="Parent">
            <Page Self="parent-page" GeometricBounds="0 0 200 200"/>{}{}
          </MasterSpread></idPkg:MasterSpread>"#,
        rectangle("parent-a", 10, ""),
        rectangle("parent-b", 60, "")
    );
    let page = |id: &str, name: &str, x: i32, list: &str| {
        format!(
            r#"<Page Self="{id}" Name="{name}" AppliedMaster="master" GeometricBounds="0 0 200 200"
              ItemTransform="1 0 0 1 {x} 0" OverrideList="{list}"/>"#
        )
    };
    let first = format!(
        r#"<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
          <Spread Self="first">{}{}{extra}</Spread></idPkg:Spread>"#,
        page("page-1", "1", 0, lists[0]),
        rectangle("copy-1", 10, attribute)
    );
    let mut nested = rectangle("copy-3", 10, "");
    for i in 0..70 {
        nested = format!(r#"<Group Self="group-{i}" ItemLayer="layer">{nested}</Group>"#);
    }
    let facing = format!(
        r#"<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
          <Spread Self="facing">{}{}{}{nested}</Spread></idPkg:Spread>"#,
        page("page-2", "2", -200, lists[1]),
        page("page-3", "3", 0, lists[2]),
        rectangle("copy-2", -190, "")
    );
    zip_files(&[("designmap.xml", designmap), ("MasterSpreads/parent.xml", &parent), ("Spreads/first.xml", &first), ("Spreads/facing.xml", &facing)])
}

fn items(d: &Document) -> HashMap<&str, &Item> {
    let mut items = HashMap::new();
    for sp in d.parents.iter().map(|s| &**s).chain(d.spreads.iter().map(|s| &**s)) {
        for it in &sp.items {
            it.walk(&mut |i| {
                items.insert(i.name.as_str(), i);
            });
        }
    }
    items
}

/// Resolve exported pairs by item labels, so id reallocation cannot hide a broken relationship.
/// An absent copy is kept as None: live copies must name an actual element's Self.
pub(super) fn exported_pairs(bytes: &[u8]) -> Vec<Vec<(String, Option<String>)>> {
    let tags = |prefix| zip_text(bytes, prefix).split('<').map(str::to_owned).collect::<Vec<_>>();
    let attribute = |tag: &str, key: &str| tag.split(&format!("{key}=\"")).nth(1).and_then(|s| s.split('"').next()).map(str::to_owned);
    let labels: HashMap<_, _> = tags("").iter().filter_map(|tag| Some((attribute(tag, "Self")?, attribute(tag, "Name")?))).collect();
    tags("Spreads/")
        .iter()
        .filter(|tag| tag.starts_with("Page "))
        .map(|tag| {
            let list = attribute(tag, "OverrideList").unwrap_or_default();
            let tokens: Vec<_> = list.split_whitespace().collect();
            assert!(tokens.len().is_multiple_of(2));
            tokens.chunks_exact(2).map(|p| (labels.get(p[0]).unwrap().clone(), labels.get(p[1]).cloned())).collect()
        })
        .collect()
}

fn exported_override_lists(bytes: &[u8]) -> Vec<Vec<String>> {
    zip_text(bytes, "Spreads/")
        .split('<')
        .filter(|tag| tag.starts_with("Page "))
        .map(|tag| {
            let tag = tag.split('>').next().unwrap();
            let list = tag.split("OverrideList=\"").nth(1).unwrap().split('"').next().unwrap();
            let tokens: Vec<_> = list.split_whitespace().map(str::to_owned).collect();
            assert!(tokens.len().is_multiple_of(2));
            tokens
        })
        .collect()
}

#[test]
fn imports_override_pairs_and_round_trips_live_copies() {
    let bytes = fixture(["parent-a copy-1 parent-b n", "parent-a copy-2", "parent-b copy-3"], "", "");
    let report = import_idml_report(&bytes, &|_| None).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let d = &report.document;
    let items = items(d);
    for (copy, parent) in [("copy-1", "parent-a"), ("copy-2", "parent-a"), ("copy-3", "parent-b")] {
        assert_eq!(items[copy].overrides, Some(items[parent].id), "{copy}");
    }
    let (a, b) = (items["parent-a"].id, items["parent-b"].id);
    assert_eq!(d.spreads[0].pages[0].overridden, [a, b]);
    assert_eq!(d.spreads[1].pages[0].overridden, [a]);
    assert_eq!(d.spreads[1].pages[1].overridden, [b]);
    assert!(!d.spreads[0].items.iter().any(|i| i.overrides == Some(b)), "n has no linked copy on page 1");
    let exported = export_idml(d);
    let pairs = exported_pairs(&exported);
    assert_eq!(
        pairs,
        [
            vec![("parent-a".into(), Some("copy-1".into())), ("parent-b".into(), None)],
            vec![("parent-a".into(), Some("copy-2".into()))],
            vec![("parent-b".into(), Some("copy-3".into()))],
        ]
    );
    // The deleted copy is n; every live pair resolves to an actual item.
    assert_eq!(exported_override_lists(&exported)[0][3], "n");
    assert_eq!(pairs.iter().flatten().filter(|(_, copy)| copy.is_none()).count(), 1);
}

#[test]
fn deleted_override_copies_export_as_n_and_round_trip() {
    // Page 1's second copy was deleted; the same parent still has a live copy on page 3.
    let source = fixture(["parent-a copy-1 parent-b n", "parent-a copy-2", "parent-b copy-3"], "", "");
    let expected = exported_pairs(&source);
    let mut d = import_idml(&source).unwrap();
    for round in 1..=2 {
        let bytes = export_idml(&d);
        let lists = exported_override_lists(&bytes);
        assert_eq!(lists[0][3], "n", "export {round}: a deleted copy stays n");
        let all = zip_text(&bytes, "");
        let self_ids: std::collections::HashSet<_> = all.split("Self=\"").skip(1).map(|s| s.split('"').next().unwrap()).collect();
        let missing: Vec<_> =
            lists.iter().flat_map(|list| list.iter().skip(1).step_by(2)).filter(|id| *id != "n" && !self_ids.contains(id.as_str())).collect();
        assert!(missing.is_empty(), "export {round}: unresolved copy ids: {missing:?}");
        assert!(!self_ids.contains("n"), "n is a sentinel, not an item id");
        assert_eq!(exported_pairs(&bytes), expected, "export {round}: live pairs and the deleted pair survive");

        let report = import_idml_report(&bytes, &|_| None).unwrap();
        assert!(report.warnings.is_empty(), "import {round}: {:?}", report.warnings);
        let linked = items(&report.document);
        for (copy, parent) in [("copy-1", "parent-a"), ("copy-2", "parent-a"), ("copy-3", "parent-b")] {
            assert_eq!(linked[copy].overrides, Some(linked[parent].id), "import {round}: {copy}");
        }
        let (a, b) = (linked["parent-a"].id, linked["parent-b"].id);
        assert_eq!(report.document.spreads[0].pages[0].overridden, [a, b]);
        assert!(!report.document.spreads[0].items.iter().any(|i| i.overrides == Some(b)), "import {round}: n has no linked copy");
        d = report.document;
    }
}

#[test]
fn override_pairs_round_trip_without_item_attributes() {
    let bytes = fixture(["parent-a copy-1", "parent-a copy-2", "parent-b copy-3"], "", "");
    let expected = exported_pairs(&bytes);
    let first = export_idml(&import_idml(&bytes).unwrap());
    assert_eq!(exported_pairs(&first), expected, "live copies retain their actual Self ids");
    let second = export_idml(&import_idml(&first).unwrap());
    assert_eq!(exported_pairs(&second), expected, "another import/export preserves the same pairs");
}

#[test]
fn override_pairs_never_link_the_deleted_n_sentinel() {
    // Even hostile input defining an item with the reserved Self=n must not turn n into a copy.
    let bytes = fixture(["parent-b n", "", ""], "", &rectangle("n", 60, ""));
    let report = import_idml_report(&bytes, &|_| None).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let items = items(&report.document);
    assert_eq!(report.document.spreads[0].pages[0].overridden, [items["parent-b"].id]);
    assert!(items.values().all(|i| i.overrides.is_none()), "n never links an item");
}

#[test]
fn override_pairs_keep_legacy_parent_lists_and_odd_lists() {
    for (list, expected) in [
        ("parent-a parent-b", vec!["parent-a", "parent-b"]),
        ("parent-b parent-a parent-b", vec!["parent-b", "parent-a", "parent-b"]),
        ("parent-a copy-1 parent-b", vec!["parent-a", "parent-b"]),
        ("copy-1 parent-a", vec!["parent-a"]),
        ("unknown copy-1", vec![]),
    ] {
        let report = import_idml_report(&fixture([list, "", ""], "", ""), &|_| None).unwrap();
        assert!(report.warnings.is_empty(), "{list}: {:?}", report.warnings);
        let items = items(&report.document);
        let expected: Vec<ItemId> = expected.into_iter().map(|name| items[name].id).collect();
        assert_eq!(report.document.spreads[0].pages[0].overridden, expected, "{list}");
        assert!(items.values().all(|i| i.overrides.is_none()), "{list}: no inferred links");
    }
}

#[test]
fn translated_empty_override_copy_stays_on_its_own_page() {
    let mut d = import_idml(&fixture(["", "parent-a copy-2", "parent-a copy-3"], "", "")).unwrap();
    let linked = items(&d);
    let mut empty = (*linked["copy-3"]).clone();
    let left = Arc::new((*linked["copy-2"]).clone());
    let sp = Arc::make_mut(&mut d.spreads[1]);
    empty.shape = Shape::Group;
    empty.content = Content::Group { items: vec![] };
    empty.path = Default::default();
    empty.xf = Affine::translate((sp.pages[1].x + 10.0, 10.0));
    // The right-page empty copy is written before the left-page nonempty copy of this parent.
    sp.items = vec![Arc::new(empty), left];

    let pairs = exported_pairs(&export_idml(&d));
    assert_eq!(pairs[1], [("parent-a".into(), Some("copy-2".into()))]);
    assert_eq!(pairs[2], [("parent-a".into(), Some("copy-3".into()))]);
}

/// The original two-pass scan, as a reference for the indexed export's exact OverrideList text.
fn legacy_override_lists(sp: &Spread) -> Vec<Vec<String>> {
    let copies: Vec<_> = sp.items.iter().filter_map(|it| Some((it.overrides?, it.id, sp.page_at_x(it.bounds().center().x)))).collect();
    let mut used = vec![false; copies.len()];
    let mut own = Vec::new();
    for (i, pg) in sp.pages.iter().enumerate() {
        let mut row = Vec::new();
        for parent in &pg.overridden {
            let k = (0..copies.len()).find(|k| !used[*k] && copies[*k].0 == *parent && copies[*k].2 == Some(i));
            if let Some(k) = k {
                used[k] = true;
            }
            row.push(k);
        }
        own.push(row);
    }
    sp.pages
        .iter()
        .enumerate()
        .map(|(i, pg)| {
            let mut list = Vec::new();
            for (j, parent) in pg.overridden.iter().enumerate() {
                let k = own[i][j].or_else(|| {
                    let k = (0..copies.len()).find(|k| !used[*k] && copies[*k].0 == *parent);
                    if let Some(k) = k {
                        used[k] = true;
                    }
                    k
                });
                list.push(crate::export::uid(parent.0));
                list.push(k.map_or_else(|| "n".into(), |k| crate::export::uid(copies[k].1.0)));
            }
            list
        })
        .collect()
}

#[test]
fn indexed_override_pairs_match_legacy_scan_with_reservations_and_fallbacks() {
    let mut d = import_idml(&fixture(["", "", ""], "", "")).unwrap();
    let linked = items(&d);
    let (a, b, layer) = (linked["parent-a"].id, linked["parent-b"].id, linked["parent-a"].layer);
    let specs = [(a, 1), (b, 0), (a, 1), (a, 0), (b, 1), (b, 0), (a, 1), (b, 0)];
    let mut copies = Vec::new();
    for (k, (parent, page)) in specs.into_iter().enumerate() {
        let x = d.spreads[1].pages[page].x + 10.0;
        let mut copy = Item::new(ItemId(d.alloc()), layer, Shape::Rectangle, shapes::rectangle(Rect::new(x, 10.0, x + 20.0, 30.0)));
        copy.name = format!("copy-{k}");
        copy.overrides = Some(parent);
        copies.push(Arc::new(copy));
    }
    let sp = Arc::make_mut(&mut d.spreads[1]);
    sp.items = copies;
    sp.pages[0].overridden = vec![a, b, a, b, a, a];
    sp.pages[1].overridden = vec![b, a, b, b, a];
    let expected = legacy_override_lists(sp);
    let bytes = export_idml(&d);
    assert_eq!(exported_override_lists(&bytes)[1..], expected);
    let pairs = exported_pairs(&bytes);
    let labels = |page: usize| pairs[page].iter().map(|(_, copy)| copy.as_deref()).collect::<Vec<_>>();
    assert_eq!(labels(1), [Some("copy-3"), Some("copy-1"), Some("copy-6"), Some("copy-5"), None, None]);
    assert_eq!(labels(2), [Some("copy-4"), Some("copy-0"), Some("copy-7"), None, Some("copy-2")]);
}

#[test]
fn override_pairs_take_precedence_over_item_attributes() {
    for attribute in [r#"OverriddenPageItem="parent-a""#, r#"OverriddenPageItem="parent-b""#] {
        let report = import_idml_report(&fixture(["parent-a copy-1", "", ""], attribute, ""), &|_| None).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let items = items(&report.document);
        assert_eq!(items["copy-1"].overrides, Some(items["parent-a"].id));
    }
    let report = import_idml_report(&fixture(["parent-a", "", ""], r#"OverriddenPageItem="parent-b""#, ""), &|_| None).unwrap();
    let items = items(&report.document);
    assert_eq!(items["copy-1"].overrides, Some(items["parent-b"].id), "legacy attribute path remains intact");
}

#[test]
fn override_pairs_keep_the_first_parent_and_warn_once_per_copy() {
    let bytes = fixture(["", "parent-a copy-2 parent-a copy-2", "parent-b copy-2 parent-b copy-2"], "", "");
    let report = import_idml_report(&bytes, &|_| None).unwrap();
    let items = items(&report.document);
    assert_eq!(items["copy-2"].overrides, Some(items["parent-a"].id));
    assert_eq!(report.document.spreads[1].pages[0].overridden, [items["parent-a"].id; 2]);
    assert_eq!(report.document.spreads[1].pages[1].overridden, [items["parent-b"].id; 2]);
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
    assert!(report.warnings[0].contains("page 3") && report.warnings[0].contains("copy-2") && report.warnings[0].contains("first pair"));
}

#[test]
fn override_pairs_with_unresolved_copies_keep_the_whole_page_legacy_and_warn() {
    for (list, count) in [
        ("parent-a copy-1 parent-b missing", 1),
        ("parent-a copy-1 parent-b copy-2", 1), // copy-2 exists on a different spread.
        ("parent-a absent parent-b missing", 2),
    ] {
        let report = import_idml_report(&fixture([list, "parent-a copy-2", ""], "", ""), &|_| None).unwrap();
        let items = items(&report.document);
        assert_eq!(items["copy-1"].overrides, None, "{list}: do not partially link a page");
        assert_eq!(items["copy-2"].overrides, Some(items["parent-a"].id), "another page still links");
        assert_eq!(report.document.spreads[0].pages[0].overridden, [items["parent-a"].id, items["parent-b"].id]);
        assert_eq!(report.warnings.len(), 1, "{list}: {:?}", report.warnings);
        let warning = &report.warnings[0];
        assert!(warning.contains("page 1") && warning.contains(&format!("{count} copy ids")), "{warning}");
    }
}

#[test]
fn override_pairs_import_twenty_thousand_copies() {
    let mut parents = String::new();
    let mut copies = String::new();
    let mut list = String::new();
    for i in 0..20_000 {
        write!(parents, r#"<Rectangle Self="parent-{i}"/>"#).unwrap();
        write!(copies, r#"<Rectangle Self="copy-{i}"/>"#).unwrap();
        write!(list, "parent-{i} copy-{i} ").unwrap();
    }
    let designmap = r#"<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
      <idPkg:MasterSpread src="MasterSpreads/parent.xml"/>
      <idPkg:Spread src="Spreads/spread.xml"/>
    </Document>"#;
    let master = format!(
        r#"<idPkg:MasterSpread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
          <MasterSpread Self="master"><Page GeometricBounds="0 0 200 200"/>{parents}</MasterSpread>
        </idPkg:MasterSpread>"#
    );
    let spread = format!(
        r#"<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
          <Spread><Page AppliedMaster="master" GeometricBounds="0 0 200 200" OverrideList="{list}"/>{copies}</Spread>
        </idPkg:Spread>"#
    );
    let bytes = zip_files(&[("designmap.xml", designmap), ("MasterSpreads/parent.xml", &master), ("Spreads/spread.xml", &spread)]);
    let report = import_idml_report(&bytes, &|_| None).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let d = &report.document;
    let parents: Vec<_> = d.parents[0].items.iter().map(|i| i.id).collect();
    let copies: Vec<_> = d.spreads[0].items.iter().map(|i| i.overrides).collect();
    assert_eq!(parents.len(), 20_000);
    assert_eq!(d.spreads[0].pages[0].overridden, parents);
    assert_eq!(copies, parents.into_iter().map(Some).collect::<Vec<_>>());
}
