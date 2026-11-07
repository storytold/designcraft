use super::*;

/// A document with one text frame carrying `label` as its Script Label and `keyed` as its keyed
/// script labels.
fn labelled(label: &str, keyed: &[(&str, &str)]) -> Document {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (id, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(40.0, 40.0, 400.0, 400.0), lid, "Labelled", ParaFormat::default()).unwrap();
    let it = d.item_mut(id).unwrap();
    it.label = label.into();
    it.labels = keyed.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    d
}

fn the_frame(d: &Document) -> &designcraft_doc::Item {
    d.spreads.iter().flat_map(|sp| sp.items.iter()).find(|it| it.text_frame().is_some()).expect("the text frame")
}

#[test]
fn keyed_script_labels_come_in_and_go_out_before_the_path() {
    let d = labelled("", &[("fit", "Proportionally"), ("imagepath", "file:/images/x & <y>.png")]);
    let bytes = export_idml(&d);
    let xml = zip_text(&bytes, "Spreads/");
    // Label precedes PathGeometry inside the item's Properties.
    let label = xml.find("<Label>").expect("a Label element");
    assert!(xml[label..].contains("<PathGeometry>"), "{xml}");
    assert!(!xml.contains(r#"Key="Label""#), "no Script Label to write: {xml}");
    let back = import_idml(&bytes).unwrap();
    let it = the_frame(&back);
    assert_eq!(it.labels.get("imagepath").map(String::as_str), Some("file:/images/x & <y>.png"));
    assert_eq!(it.labels.len(), 2);
    assert_eq!(it.label, "");
}

#[test]
fn the_script_label_is_the_key_value_pair_keyed_label() {
    // InDesign keeps an object's Script Label as `<KeyValuePair Key="Label" …/>` among its keyed
    // labels: it comes in as the Script Label (which `object.findByLabel` searches), not as a
    // keyed label, and goes out the same way.
    let d = labelled("Spine", &[("fit", "Proportionally")]);
    let bytes = export_idml(&d);
    let xml = zip_text(&bytes, "Spreads/");
    assert_eq!(xml.matches(r#"<KeyValuePair Key="Label" Value="Spine"/>"#).count(), 1, "{xml}");
    let back = import_idml(&bytes).unwrap();
    let it = the_frame(&back);
    assert_eq!(it.label, "Spine");
    assert_eq!(it.labels.keys().collect::<Vec<_>>(), ["fit"]);
}

#[test]
fn a_keyed_label_under_the_script_labels_key_gives_way_to_the_script_label() {
    let d = labelled("Spine", &[("Label", "keyed")]);
    let xml = zip_text(&export_idml(&d), "Spreads/");
    assert_eq!(xml.matches(r#"Key="Label""#).count(), 1, "one pair per key: {xml}");
    assert!(xml.contains(r#"<KeyValuePair Key="Label" Value="Spine"/>"#), "{xml}");
    // Without a Script Label, the keyed one is written, and comes back as the Script Label.
    let d = labelled("", &[("Label", "keyed")]);
    let back = import_idml(&export_idml(&d)).unwrap();
    assert_eq!(the_frame(&back).label, "keyed");
}
