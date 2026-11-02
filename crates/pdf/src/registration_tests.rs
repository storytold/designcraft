use super::{
    Affine, Arc, AssetId, BookletOptions, Cache, Content, Document, Item, PdfOptions, Rect, SpreadRef, Standard, export_booklet, export_pdf,
    merge_pdfs,
};

fn placed_document(source: Vec<u8>) -> Document {
    use designcraft_doc::{Asset, Graphic, ItemId, Shape};
    let mut doc = Document::new(&Default::default());
    let asset = AssetId(doc.alloc());
    doc.add_asset(Asset { id: asset, name: "placed.pdf".into(), mime: "application/pdf".into(), data: Arc::new(source), ..Default::default() });
    let mut item =
        Item::new(ItemId(doc.alloc()), doc.default_layer(), Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, 100.0, 100.0)));
    item.content = Content::Graphic(Graphic {
        asset,
        size: (100.0, 100.0),
        xf: Affine::IDENTITY,
        clip: None,
        auto_fit: Default::default(),
        fit_align: 4,
        crop: [0.0; 4],
        wrap: Default::default(),
        fill: Default::default(),
        layer_options: None,
    });
    doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
    doc
}

fn layered_source(catalog: bool) -> Vec<u8> {
    let properties = if catalog { "/OCProperties<</OCGs[5 0 R]/D<</OFF[5 0 R]>>>>" } else { "" };
    let content = b"/OC /a BDC 0.125 0.625 0.875 rg 0 0 100 100 re f EMC";
    let objects = [
        format!("<</Type/Catalog/Pages 2 0 R{properties}>>"),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".into(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Resources<</Properties<</a 5 0 R>>>>/Contents 4 0 R>>".into(),
        format!("<</Length {}>>\nstream\n{}\nendstream", content.len(), String::from_utf8_lossy(content)),
        "<</Type/OCG/Name(Hidden)>>".into(),
    ];
    assemble(&objects)
}

fn unlayered_source() -> Vec<u8> {
    let content = "0.125 0.625 0.875 rg 0 0 100 100 re f";
    assemble(&[
        "<</Type/Catalog/Pages 2 0 R>>".into(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".into(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Resources<<>>/Contents 4 0 R>>".into(),
        format!("<</Length {}>>\nstream\n{content}\nendstream", content.len()),
    ])
}

pub(super) fn assemble(objects: &[String]) -> Vec<u8> {
    let mut bytes = b"%PDF-1.5\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
    }
    let xref = bytes.len();
    let size = objects.len() + 1;
    bytes.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n\r\n").as_bytes());
    }
    bytes.extend_from_slice(format!("trailer\n<</Size {size}/Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
    bytes
}

thread_local! {
    pub(crate) static REGISTRATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn registrations(action: impl FnOnce()) -> usize {
    REGISTRATIONS.set(0);
    action();
    REGISTRATIONS.get()
}

fn check_vector_artwork(pdf: &hayro_syntax::Pdf) {
    use hayro_syntax::object::{Name, Object};

    let mut found = false;
    for object in pdf.objects() {
        let Object::Stream(stream) = object else { continue };
        let subtype = stream.dict().get::<Name<'_>>(b"Subtype");
        assert_ne!(subtype.as_ref().map(AsRef::as_ref), Some(b"Image".as_slice()), "placed artwork must remain vector");
        if subtype.as_ref().map(AsRef::as_ref) == Some(b"Form".as_slice()) {
            let content = stream.decoded().unwrap();
            let content = String::from_utf8_lossy(&content);
            found |= content.contains("0.125 0.625 0.875 rg") && content.contains("0 0 100 100 re f");
        }
    }
    assert!(found, "placed rectangle and its distinctive fill colour must survive in a vector Form XObject");
}

fn check_unlayered_pdf(bytes: &[u8], pages: usize, artwork: bool) {
    use hayro_syntax::object::Dict;

    let pdf = hayro_syntax::Pdf::new(bytes.to_vec()).unwrap();
    assert_eq!(pdf.pages().len(), pages);
    let catalog = pdf.xref().get::<Dict<'_>>(pdf.xref().root_id()).unwrap();
    assert!(!catalog.contains_key(b"OCProperties"), "unlayered output must not acquire optional content");
    if artwork {
        check_vector_artwork(&pdf);
    }
}

fn check_unlayered(doc: &Document, artwork: bool) {
    let cache = Cache::new();
    let mut bytes = Vec::new();
    assert_eq!(
        registrations(|| {
            bytes = export_pdf(doc, &cache, &PdfOptions::default()).unwrap();
            check_unlayered_pdf(&bytes, 1, artwork);
        }),
        0
    );
    assert_eq!(
        registrations(|| {
            let report = export_booklet(doc, &cache, &BookletOptions::default()).unwrap();
            check_unlayered_pdf(&report.bytes, 2, artwork);
        }),
        0
    );
    assert_eq!(
        registrations(|| {
            let merged = merge_pdfs(&[(bytes, vec![(100.0, 100.0)])], None).unwrap();
            check_unlayered_pdf(&merged, 1, artwork);
        }),
        0
    );
}

#[test]
fn no_placed_pdfs_skip_optional_content_registration() {
    check_unlayered(&Document::new(&Default::default()), false);
}

#[test]
fn unlayered_placed_pdf_skips_optional_content_registration() {
    check_unlayered(&placed_document(unlayered_source()), true);
}

#[test]
fn null_optional_content_properties_are_no_optional_content() {
    let content = "0.125 0.625 0.875 rg 0 0 100 100 re f";
    for (properties, extra) in [("/OCProperties null", None), ("/OCProperties 5 0 R", Some("null"))] {
        let mut objects = vec![
            format!("<</Type/Catalog/Pages 2 0 R{properties}>>"),
            "<</Type/Pages/Kids[3 0 R]/Count 1>>".into(),
            "<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Resources<<>>/Contents 4 0 R>>".into(),
            format!("<</Length {}>>\nstream\n{content}\nendstream", content.len()),
        ];
        objects.extend(extra.map(String::from));
        let doc = placed_document(assemble(&objects));
        for standard in [Standard::None, Standard::PdfX4] {
            let bytes = export_pdf(&doc, &Cache::new(), &PdfOptions { standard, ..Default::default() }).unwrap();
            check_unlayered_pdf(&bytes, 1, true);
        }
    }
}

#[test]
fn layered_and_resource_only_inputs_register_optional_content() {
    for hidden in [true, false] {
        let source = layered_source(hidden);
        let doc = placed_document(source.clone());
        let cache = Cache::new();
        let mut exported = Vec::new();
        assert_eq!(
            registrations(|| {
                exported = export_pdf(&doc, &cache, &PdfOptions::default()).unwrap();
            }),
            1
        );
        check_registered(&exported, hidden);
        assert_eq!(
            registrations(|| {
                let report = export_booklet(&doc, &cache, &BookletOptions::default()).unwrap();
                check_registered(&report.bytes, hidden);
            }),
            1
        );
        for input in [source, exported] {
            assert_eq!(
                registrations(|| {
                    let merged = merge_pdfs(&[(input, vec![(100.0, 100.0)])], None).unwrap();
                    check_registered(&merged, hidden);
                }),
                1
            );
        }
    }
}

fn check_registered(bytes: &[u8], hidden: bool) {
    use hayro_syntax::object::{Array, Dict, Name, Object, ObjectIdentifier};

    let pdf = hayro_syntax::Pdf::new(bytes.to_vec()).unwrap();
    check_vector_artwork(&pdf);
    let xref = pdf.xref();
    let catalog = xref.get::<Dict<'_>>(xref.root_id()).unwrap();
    let properties = catalog.get::<Dict<'_>>(b"OCProperties").unwrap();
    let groups = properties.get::<Array<'_>>(b"OCGs").unwrap();
    assert_eq!(groups.raw_iter().count(), 1);
    let group_id: ObjectIdentifier = groups.raw_iter().next().unwrap().as_obj_ref().unwrap().into();
    let group = xref.get::<Dict<'_>>(group_id).unwrap();
    assert_eq!(group.get::<Name<'_>>(b"Type").unwrap().as_ref(), b"OCG");
    assert_eq!(group.get::<hayro_syntax::object::String<'_>>(b"Name").unwrap().as_bytes(), b"Hidden");

    let config = properties.get::<Dict<'_>>(b"D").unwrap();
    if hidden {
        let off = config.get::<Array<'_>>(b"OFF").unwrap();
        assert_eq!(off.raw_iter().count(), 1);
        let off_id: ObjectIdentifier = off.raw_iter().next().unwrap().as_obj_ref().unwrap().into();
        assert_eq!(off_id, group_id, "OFF must reference the Hidden OCG listed in OCGs");
    } else if config.contains_key(b"OFF") {
        assert_eq!(config.get::<Array<'_>>(b"OFF").unwrap().raw_iter().count(), 0);
    }
    // Superseded revisions may retain the marker; readers use only the objects resolved by the current xref.
    for object in pdf.objects() {
        let Object::Dict(dict) = object else { continue };
        if dict.get::<Name<'_>>(b"Type").as_ref().map(AsRef::as_ref) == Some(b"OCG") {
            assert!(!dict.contains_key(b"DesignCraftHiddenOCG"), "internal marker remains in resolved OCG {:?}", dict.obj_id());
        }
    }
}
