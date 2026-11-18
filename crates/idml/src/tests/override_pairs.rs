use std::collections::HashMap;
use std::fmt::Write as _;

use super::*;
use designcraft_doc::{Item, ItemId};

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
    zip_files(&[
        ("designmap.xml", designmap),
        ("MasterSpreads/parent.xml", &parent),
        ("Spreads/first.xml", &first),
        ("Spreads/facing.xml", &facing),
    ])
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
    let pairs = exported_pairs(&export_idml(d));
    assert_eq!(
        pairs,
        [
            vec![("parent-a".into(), Some("copy-1".into())), ("parent-b".into(), None)],
            vec![("parent-a".into(), Some("copy-2".into()))],
            vec![("parent-b".into(), Some("copy-3".into()))],
        ]
    );
    // The unchanged exporter uses an unused id for the deleted copy; every live pair resolves.
    assert_eq!(pairs.iter().flatten().filter(|(_, copy)| copy.is_none()).count(), 1);
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
        let items = items(&report.document);
        let expected: Vec<ItemId> = expected.into_iter().map(|name| items[name].id).collect();
        assert_eq!(report.document.spreads[0].pages[0].overridden, expected, "{list}");
        assert!(items.values().all(|i| i.overrides.is_none()), "{list}: no inferred links");
    }
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
